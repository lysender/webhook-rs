use axum::body::to_bytes;
use axum::body::Body;
use axum::extract::ws::Message;
use axum::extract::ws::WebSocket;
use axum::extract::WebSocketUpgrade;
use axum::extract::{FromRef, Request, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::response::Response;
use axum::routing::any;
use axum::routing::get;
use axum::Router;
use futures::stream::SplitSink;
use futures::stream::SplitStream;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpListener;
use tokio::sync::Mutex;
use tokio::time::sleep;
use tower::ServiceBuilder;
use tower_http::trace::{DefaultMakeSpan, DefaultOnResponse, TraceLayer};
use tracing::{error, info, Level};
use uuid::Uuid;

// Allows to extract the IP of connecting user
use axum::extract::connect_info::ConnectInfo;

// Allows to split the websocket stream into separate TX and RX branches
use futures::{sink::SinkExt, stream::StreamExt};

use crate::context::ServerContext;
use crate::message::HttpLine;
use crate::message::StatusLine;
use crate::message::TunnelMessage2;
use crate::message::WebhookHeader;
use crate::token::verify_auth_token;
use crate::Error;
use crate::Result;

pub struct TunnelReceiver {
    stream: Option<SplitStream<WebSocket>>,
}

impl TunnelReceiver {
    pub fn new(stream: SplitStream<WebSocket>) -> Self {
        Self {
            stream: Some(stream),
        }
    }

    pub async fn read(&mut self) -> Option<Message> {
        if let Some(stream) = self.stream.as_mut() {
            if let Some(Ok(msg)) = stream.next().await {
                return Some(msg);
            }
        }
        None
    }
}

pub struct TunnelSender {
    stream: Option<SplitSink<WebSocket, Message>>,
}

impl TunnelSender {
    pub fn new(stream: SplitSink<WebSocket, Message>) -> Self {
        Self {
            stream: Some(stream),
        }
    }

    pub async fn send(&mut self, msg: Message) -> Result<()> {
        if let Some(stream) = self.stream.as_mut() {
            if let Err(e) = stream.send(msg).await {
                let msg = format!("Error sending message: {e}");
                return Err(msg.into());
            }
        }
        Ok(())
    }
}

#[derive(Clone, FromRef)]
pub struct AppState {
    ctx: Arc<ServerContext>,
}

pub async fn start_web_server(ctx: Arc<ServerContext>) -> Result<()> {
    let arc_config = ctx.config.clone();
    let web_address = arc_config.web_address.clone();
    let webhook_path = arc_config.webhook_path.clone();

    let state = AppState { ctx: ctx.clone() };

    let wh_path = webhook_path.as_str();
    let routes = if wh_path == "*" {
        // Website mode
        Router::new()
            .route("/_ws", any(ws_handler))
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
            .route("/_ws", any(ws_handler))
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
    axum::serve(
        listener,
        routes.into_make_service_with_connect_info::<SocketAddr>(),
    )
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
    let ctx = state.ctx.clone();
    let id = Uuid::now_v7();

    if !ctx.is_verified().await {
        return handle_forward_error(None);
    }

    let uri = request.uri().to_string();
    let method = request.method().to_string();

    // Build original request
    let wh_header = WebhookHeader::new(id);
    let http_st = StatusLine::Request(HttpLine::new(method.clone(), uri, "HTTP/1.1".to_string()));
    let mut http_req = TunnelMessage2::new(wh_header, http_st);

    // Add original headers
    http_req.http_headers.extend(
        request
            .headers()
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_str().unwrap().to_string())),
    );

    // Add original body if present
    let with_body = vec!["POST", "PUT", "PATCH"];
    if with_body.contains(&method.as_str()) {
        let body_bytes = to_bytes(request.into_body(), usize::MAX).await.unwrap();
        http_req.http_body = body_bytes.to_vec();
    }

    // Push the request to the queue
    ctx.add_request(http_req).await;

    // Wait for response to arrive
    let tunnel_res = ctx.get_response(&id.as_u128()).await;

    match tunnel_res {
        Ok(res) => handle_forward_success(res),
        Err(e) => handle_forward_error(Some(e)),
    }
}

fn handle_forward_success(fw_res: TunnelMessage2) -> Response<Body> {
    let st_opt = match fw_res.http_line {
        StatusLine::Response(st) => Some(st),
        _ => None,
    };

    let st = st_opt.unwrap();

    let mut r = Response::builder().status(st.status_code);
    for (k, v) in fw_res.http_headers.iter() {
        r = r.header(k, v);
    }

    r.body(Body::from(fw_res.http_body)).unwrap()
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

async fn ws_handler(
    state: State<AppState>,
    ws: WebSocketUpgrade,
    headers: HeaderMap,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
) -> Response<Body> {
    let ctx = state.ctx.clone();

    // Find auth token
    let Some(auth_token) = headers.get("authorization") else {
        return (StatusCode::UNAUTHORIZED, "Unauthorized").into_response();
    };

    let secret = ctx.config.jwt_secret.as_str();
    let auth_token = auth_token.to_str().unwrap();
    if let Err(_) = verify_auth_token(auth_token, secret) {
        return (StatusCode::UNAUTHORIZED, "Unauthorized").into_response();
    }

    ws.on_upgrade(move |socket| handle_socket(ctx.clone(), socket, addr))
}

async fn handle_socket(ctx: Arc<ServerContext>, socket: WebSocket, who: SocketAddr) {
    // We know, at this point, that there is a connected client
    ctx.verify().await;

    // Split reader and writer
    let (sender, receiver) = socket.split();

    let tunnel_receiver = Arc::new(Mutex::new(TunnelReceiver::new(receiver)));
    let tunnel_sender = Arc::new(Mutex::new(TunnelSender::new(sender)));

    let join_res = tokio::try_join!(
        ping_task(tunnel_sender.clone()),
        send_task(ctx.clone(), tunnel_sender),
        recv_task(ctx.clone(), tunnel_receiver)
    );

    if let Err(e) = join_res {
        let msg = format!("{}", e);
        error!(msg);
    }

    ctx.unverify().await;

    // Returning from the handler closes the websocket connection
    info!("Websocket context {:?} closed", who);
}

async fn ping_task(sender: Arc<Mutex<TunnelSender>>) -> Result<()> {
    loop {
        // Send a ping message
        let res = {
            let mut stream = sender.lock().await;
            stream.send(Message::Ping(vec![1, 2, 3])).await
        };

        if let Err(e) = res {
            error!("Failed to send ping message to client: {}", e);
            break;
        }

        // Send another in 60 seconds
        sleep(Duration::from_secs(60)).await;
    }

    Err("Ping task ended".into())
}

async fn send_task(ctx: Arc<ServerContext>, sender: Arc<Mutex<TunnelSender>>) -> Result<()> {
    loop {
        if let Some(message) = ctx.get_request().await {
            let res = {
                let mut stream = sender.lock().await;
                stream.send(Message::Binary(message.into_bytes())).await
            };

            if let Err(e) = res {
                error!("Failed to send message to client: {}", e);
                break;
            }
        }
    }

    Ok(())
}

async fn recv_task(ctx: Arc<ServerContext>, receiver: Arc<Mutex<TunnelReceiver>>) -> Result<()> {
    loop {
        let received = {
            let mut stream = receiver.lock().await;
            stream.read().await
        };

        if let Some(msg) = received {
            let res = handle_ws_message(ctx.clone(), msg).await;
            if let Err(e) = res {
                error!("Failed to handle message from client: {}", e);
            }
        }
    }

    Ok(())
}

async fn handle_ws_message(ctx: Arc<ServerContext>, msg: Message) -> Result<()> {
    match msg {
        Message::Pong(v) => {
            // Track the last pong message timestamp
            info!("Received pong with {:?}", v);
            Ok(())
        }
        Message::Binary(b) => match TunnelMessage2::from_buffer(&b[..]) {
            Ok(message) => {
                ctx.add_response(message).await;
                Ok(())
            }
            Err(e) => Err(e),
        },
        _ => {
            // Do nothing
            Ok(())
        }
    }
}
