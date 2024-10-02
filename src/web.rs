use axum::body::to_bytes;
use axum::body::Body;
use axum::extract::{FromRef, Request, State};
use axum::response::Response;
use axum::routing::get;
use axum::Router;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::Mutex;
use tower::ServiceBuilder;
use tower_http::trace::{DefaultMakeSpan, DefaultOnResponse, TraceLayer};
use tracing::{info, Level};

use crate::config::ServerConfig;
use crate::message::RequestLine;
use crate::message::StatusLine;
use crate::message::TunnelMessage;
use crate::message::WEBHOOK_OP_FORWARD;
use crate::message::X_WEEB_HOOK_OP;
use crate::tunnel::TunnelClient;
use crate::Error;
use crate::Result;

#[derive(Clone, FromRef)]
pub struct AppState {
    tunnel: Arc<Mutex<TunnelClient>>,
    config: Arc<ServerConfig>,
}

pub async fn start_web_server(
    tunnel: Arc<Mutex<TunnelClient>>,
    config: Arc<ServerConfig>,
) -> Result<()> {
    let arc_tunnel = tunnel.clone();
    let arc_config = config.clone();
    let web_address = arc_config.web_address.clone();
    let webhook_path = arc_config.webhook_path.clone();

    let state = AppState {
        tunnel: arc_tunnel,
        config: arc_config,
    };

    let routes = Router::new()
        .route("/", get(index_handler))
        .route(
            webhook_path.as_str(),
            get(webhook_handler)
                .post(webhook_handler)
                .put(webhook_handler)
                .patch(webhook_handler),
        )
        .fallback(fallback_handler)
        .with_state(state)
        .layer(
            ServiceBuilder::new().layer(
                TraceLayer::new_for_http()
                    .make_span_with(DefaultMakeSpan::new().level(Level::INFO))
                    .on_response(DefaultOnResponse::new().level(Level::INFO)),
            ),
        );

    // Setup the server
    info!("HTTP server started at {}", web_address);

    let listener = TcpListener::bind(web_address).await.unwrap();
    axum::serve(listener, routes.into_make_service())
        .await
        .unwrap();

    Ok(())
}

async fn index_handler() -> Response<Body> {
    Response::builder()
        .status(200)
        .body(Body::from("OK"))
        .unwrap()
}

async fn fallback_handler() -> Response<Body> {
    Response::builder()
        .status(404)
        .body(Body::from("NOT FOUND"))
        .unwrap()
}

async fn webhook_handler(state: State<AppState>, request: Request) -> Response<Body> {
    let mut client = state.tunnel.lock().await;
    if !client.is_verified() {
        return handle_forward_error(None);
    }

    let uri = request.uri().to_string();
    let method = request.method().to_string();

    // Build original request
    let http_st = StatusLine::Request(RequestLine::new(
        method.clone(),
        uri,
        "HTTP/1.1".to_string(),
    ));
    let mut http_req = TunnelMessage::new(http_st);

    // Add original headers
    http_req.headers = request
        .headers()
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_str().unwrap().to_string()))
        .collect();

    // Add custom headers for forwarding
    http_req
        .headers
        .push((X_WEEB_HOOK_OP.to_string(), WEBHOOK_OP_FORWARD.to_string()));

    // Add original body if present
    let with_body = vec!["POST", "PUT", "PATCH"];
    if with_body.contains(&method.as_str()) {
        let body_bytes = to_bytes(request.into_body(), usize::MAX).await.unwrap();
        http_req.initial_body = body_bytes.to_vec();
    }

    if let Err(write_err) = client.write(&http_req.into_bytes()).await {
        return handle_forward_error(Some(write_err));
    }

    // Read from client response
    let mut buffer = [0; 4096];
    let mut tunnel_res: Option<TunnelMessage> = None;

    loop {
        info!("Waiting for tunnel response, looping...");

        match client.read(&mut buffer).await {
            Ok(0) => {
                return handle_forward_error(Some(Error::AnyError(
                    "No response from connected client.".to_string(),
                )));
            }
            Ok(n) => {
                if let Some(mut res) = tunnel_res.take() {
                    info!("Appending data to existing response.");
                    // Append data to existing body, assuming these are part of the data
                    if res.accumulate_body(&buffer[..n]) {
                        // Body complete, let's process the message
                        break;
                    }
                } else {
                    // This is a fresh buffer, read headers
                    let fresh_buffer = TunnelMessage::from_buffer(&buffer[..n]);
                    match fresh_buffer {
                        Ok(res) => {
                            let complete = res.complete;
                            tunnel_res = Some(res);

                            if complete {
                                break;
                            }
                        }
                        Err(e) => {
                            return handle_forward_error(Some(e));
                        }
                    };
                }
            }
            Err(e) => {
                let msg = format!("Error reading back from connected client: {}", e);
                return handle_forward_error(Some(Error::AnyError(msg.to_string())));
            }
        };
    }

    if let Some(fw_res) = tunnel_res {
        return handle_forward_success(fw_res);
    } else {
        return handle_forward_error(Some(Error::AnyError(
            "No response from connected client.".to_string(),
        )));
    }
}

fn handle_forward_success(fw_res: TunnelMessage) -> Response<Body> {
    let st_opt = match fw_res.status_line {
        StatusLine::Response(st) => Some(st),
        _ => None,
    };

    let st = st_opt.unwrap();

    let mut r = Response::builder().status(st.status_code);
    for (k, v) in fw_res.headers.iter() {
        r = r.header(k, v);
    }

    r.body(Body::from(fw_res.initial_body)).unwrap()
}

fn handle_forward_error(error: Option<Error>) -> Response<Body> {
    let write_err: String = match error {
        Some(err) => format!(": {}", err),
        None => "".to_string(),
    };
    let contents = format!("Service Unavailable{}\n", write_err);
    Response::builder()
        .status(503)
        .body(Body::from(contents))
        .unwrap()
}
