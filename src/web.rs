use axum::body::to_bytes;
use axum::extract::{FromRef, Request, State};
use axum::http::Method;
use axum::response::Response;
use axum::routing::get;
use axum::Router;
use axum::{body::Body, http::HeaderMap};
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt};
use tokio::sync::Mutex;
use tokio::{
    io::BufReader,
    net::{TcpListener, TcpStream},
};
use tower::ServiceBuilder;
use tower_http::trace::{DefaultMakeSpan, DefaultOnResponse, TraceLayer};
use tracing::{error, info, Level};

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

async fn webhook_handler(
    state: State<AppState>,
    request: Request,
    //headers: HeaderMap,
    //method: Method,
    //body: Body,
) -> Response<Body> {
    let mut client = state.tunnel.lock().await;
    if client.is_verified() {
        // How do we send the original request to the connected client?
        // FORWARD /webhook WEBHOOK/1.0
        // \r\n
        // \r\n
        // -- Original request here as bytes?
        // Something like that?
        // Then maybe we encode the whoe request line plus body as base64 encoded data?
        // Or maybe just end it raw?
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
            return handle_forward_success();
        }
    }

    handle_forward_error(None)
}

fn handle_forward_success() -> Response<Body> {
    Response::builder()
        .status(200)
        .body(Body::from("OK"))
        .unwrap()
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
