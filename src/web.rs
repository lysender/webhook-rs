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
    let content = Body::from("OK");
    let content_type = "text/plain; charset=UTF8";
    Response::builder()
        .header("Content-Type", content_type)
        .status(200)
        .body(content)
        .unwrap()
}

async fn fallback_handler() -> Response<Body> {
    let content = Body::from("NOT FOUND");
    let content_type = "text/plain; charset=UTF8";
    Response::builder()
        .header("Content-Type", content_type)
        .status(404)
        .body(content)
        .unwrap()
}

async fn webhook_handler(state: State<AppState>, request: Request) -> Response<Body> {
    let mut client = state.tunnel.lock().await;
    if client.is_verified() {
        let method = request.method().to_string();

        let mut request_bytes: Vec<u8> = Vec::new();

        // Webhook header and a separator
        request_bytes.extend_from_slice("FORWARD /webhook WEBHOOK/1.0\r\n\r\n".as_bytes());

        // Build original request
        let request_line = format!("{} {} HTTP/1.1\r\n", &method, request.uri());
        request_bytes.extend_from_slice(&request_line.as_bytes());

        for (key, value) in request.headers().iter() {
            let header_line = format!("{}: {}\r\n", key, value.to_str().unwrap());
            request_bytes.extend_from_slice(&header_line.as_bytes());
        }

        let with_body = vec!["POST", "PUT", "PATCH"];
        if with_body.contains(&method.as_str()) {
            // Add separator for the body
            request_bytes.extend_from_slice("\r\n".as_bytes());

            let body_bytes = to_bytes(request.into_body(), usize::MAX).await.unwrap();
            request_bytes.extend_from_slice(&body_bytes);
        }

        if let Err(write_err) = client.write(&request_bytes).await {
            return handle_forward_error(Some(write_err));
        } else {
            // Read from client response
            // TODO: handle when data won't fit in the buffer
            let mut buffer = [0; 4096];
            match client.read(&mut buffer).await {
                Ok(0) => {
                    // Connection closed
                    return handle_forward_error(Some(Error::AnyError(
                        "Connection closed by connected client.".to_string(),
                    )));
                }
                Ok(n) => {
                    let fw_res = String::from_utf8_lossy(&buffer[..n]).to_string();
                    return handle_forward_success(fw_res);
                }
                Err(e) => {
                    let msg = format!("Error reading back from connected client: {}", e);
                    return handle_forward_error(Some(Error::AnyError(msg.to_string())));
                }
            };
        }
    }

    handle_forward_error(None)
}

fn handle_forward_success(fw_res: String) -> Response<Body> {
    // Parse response
    let mut rn_lines = fw_res.split("\r\n");
    let response_line = rn_lines.next().unwrap();
    // Figure out the status code
    //
    let mut rl_parts = response_line.split_whitespace();
    let _ = rl_parts.next().unwrap();
    let code = rl_parts.next().unwrap();
    let status: u16 = code.to_string().parse().unwrap();

    let mut r = Response::builder().status(status);

    let mut is_body = false;
    let mut body: Vec<u8> = Vec::new();

    // Inject headers
    for header_line in rn_lines {
        if header_line == "" {
            // Could be the end of headers
            // body next...
            is_body = true;
            continue;
        }

        if is_body {
            let line = header_line.to_string();
            body.extend_from_slice(&line.as_bytes());
        } else {
            // Inject headers
            let mut h_parts = header_line.split(": ");
            let h_k = h_parts.next().unwrap();
            let h_v = h_parts.next().unwrap();

            r = r.header(h_k, h_v);
        }
    }

    r.body(body.into()).unwrap()
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
