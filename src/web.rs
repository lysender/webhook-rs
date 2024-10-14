use axum::body::to_bytes;
use axum::body::Body;
use axum::extract::{FromRef, Request, State};
use axum::response::Response;
use axum::routing::get;
use axum::Router;
use std::sync::Arc;
use tokio::net::TcpListener;
use tower::ServiceBuilder;
use tower_http::trace::{DefaultMakeSpan, DefaultOnResponse, TraceLayer};
use tracing::{info, Level};
use uuid::Uuid;

use crate::config::ServerConfig;
use crate::message::RequestLine;
use crate::message::StatusLine;
use crate::message::TunnelMessage;
use crate::message::WEBHOOK_OP;
use crate::message::WEBHOOK_OP_FORWARD;
use crate::message::WEBHOOK_REQ_ID;
use crate::queue::MessageMap;
use crate::queue::MessageQueue;
use crate::tunnel::TunnelState;
use crate::Error;
use crate::Result;

#[derive(Clone, FromRef)]
pub struct AppState {
    config: Arc<ServerConfig>,
    tunnel_state: Arc<TunnelState>,
    req_queue: Arc<MessageQueue>,
    res_map: Arc<MessageMap>,
}

pub async fn start_web_server(
    config: Arc<ServerConfig>,
    tunnel_state: Arc<TunnelState>,
    req_queue: Arc<MessageQueue>,
    res_map: Arc<MessageMap>,
) -> Result<()> {
    let arc_config = config.clone();
    let web_address = arc_config.web_address.clone();
    let webhook_path = arc_config.webhook_path.clone();

    let state = AppState {
        config: arc_config,
        tunnel_state: tunnel_state.clone(),
        req_queue: req_queue.clone(),
        res_map: res_map.clone(),
    };

    let wh_path = webhook_path.as_str();
    let routes = if wh_path == "*" {
        // Website mode
        Router::new()
            .fallback(webhook_handler)
            .with_state(state)
            .layer(
                ServiceBuilder::new().layer(
                    TraceLayer::new_for_http()
                        .make_span_with(DefaultMakeSpan::new().level(Level::INFO))
                        .on_response(DefaultOnResponse::new().level(Level::INFO)),
                ),
            )
    } else {
        // Regular webhook mode
        Router::new()
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
            )
    };

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
    let id = Uuid::now_v7();
    info!("Received webhook request: {}", id.to_string());

    if !state.tunnel_state.is_verified().await {
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
    let mut http_req = TunnelMessage::new(id, http_st);

    // Add original headers
    http_req.headers.extend(
        request
            .headers()
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_str().unwrap().to_string())),
    );

    // Add custom headers for forwarding
    http_req
        .headers
        .push((WEBHOOK_OP.to_string(), WEBHOOK_OP_FORWARD.to_string()));

    // Add original body if present
    let with_body = vec!["POST", "PUT", "PATCH"];
    if with_body.contains(&method.as_str()) {
        let body_bytes = to_bytes(request.into_body(), usize::MAX).await.unwrap();
        http_req.initial_body = body_bytes.to_vec();
    }

    // Push the request to the queue
    state.req_queue.push(http_req).await;

    // Wait for response to arrive
    let tunnel_res = state.res_map.get(&id.as_u128()).await;

    match tunnel_res {
        Ok(res) => handle_forward_success(res),
        Err(e) => handle_forward_error(Some(e)),
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
        // Skip custom headers
        if k == WEBHOOK_OP || k == WEBHOOK_REQ_ID {
            continue;
        }

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
