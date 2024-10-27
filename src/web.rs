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
use std::borrow::Cow;
use std::net::SocketAddr;
use std::ops::ControlFlow;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpListener;
use tokio::time::sleep;
use tower::ServiceBuilder;
use tower_http::trace::{DefaultMakeSpan, DefaultOnResponse, TraceLayer};
use tracing::{error, info, Level};
use uuid::Uuid;

//allows to extract the IP of connecting user
use axum::extract::connect_info::ConnectInfo;
use axum::extract::ws::CloseFrame;

//allows to split the websocket stream into separate TX and RX branches
use futures::{sink::SinkExt, stream::StreamExt};

use crate::context::ServerContext;
use crate::message::HttpLine;
use crate::message::StatusLine;
use crate::message::TunnelMessage;
use crate::message::WEBHOOK_OP;
use crate::message::WEBHOOK_OP_FORWARD;
use crate::message::WEBHOOK_REQ_ID;
use crate::token::verify_auth_token;
use crate::Error;
use crate::Result;

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
    let http_st = StatusLine::Request(HttpLine::new(method.clone(), uri, "HTTP/1.1".to_string()));
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
    ctx.add_request(http_req).await;

    // Wait for response to arrive
    let tunnel_res = ctx.get_response(&id.as_u128()).await;

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

async fn ws_handler(
    state: State<AppState>,
    ws: WebSocketUpgrade,
    headers: HeaderMap,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
) -> Response<Body> {
    let ctx = state.ctx.clone();

    println!("headers: {:?}", headers);
    // Find auth token
    let Some(auth_token) = headers.get("authorization") else {
        println!("No authorization header found");
        return (StatusCode::UNAUTHORIZED, "Unauthorized").into_response();
    };

    let secret = ctx.config.jwt_secret.as_str();
    let auth_token = auth_token.to_str().unwrap();
    if let Err(_) = verify_auth_token(auth_token, secret) {
        println!("Invalid authorization token");
        return (StatusCode::UNAUTHORIZED, "Unauthorized").into_response();
    }

    ws.on_upgrade(move |socket| handle_socket(ctx.clone(), socket, addr))
}

async fn handle_socket(ctx: Arc<ServerContext>, mut socket: WebSocket, who: SocketAddr) {
    // Send a ping
    if socket.send(Message::Ping(vec![1, 2, 3])).await.is_ok() {
        info!("Pinged {:?}", who);
    } else {
        error!("Could not ping {:?}, closing connection", who);
        return;
    }

    // Wait for a single message fron the client
    // This blocks until the client sends a message
    if let Some(msg) = socket.recv().await {
        if let Ok(msg) = msg {
            if process_msg(msg, who).is_break() {
                return;
            }
        } else {
            println!("client {:?} abruptly disconnected", who);
        }
    }

    // Send some messages with delay each message
    for i in 1..5 {
        if socket
            .send(Message::Text(format!("Hi {} times!", i)))
            .await
            .is_err()
        {
            println!("client {:?} abruptly disconnected", who);
            return;
        }
        sleep(Duration::from_millis(100)).await;
    }

    // Split reader and writer
    let (mut sender, mut receiver) = socket.split();

    // Spawn a task to send messages to the client
    let mut send_task = tokio::spawn(async move {
        let n_msg = 20;
        for i in 0..n_msg {
            // In case of a websocket error, we exit
            if sender
                .send(Message::Text(format!("Server message {}", i)))
                .await
                .is_err()
            {
                return i;
            }

            sleep(Duration::from_millis(300)).await;
        }

        println!("Sending close to {:?}", who);
        if let Err(e) = sender
            .send(Message::Close(Some(CloseFrame {
                code: axum::extract::ws::close_code::NORMAL,
                reason: Cow::from("Goodbye"),
            })))
            .await
        {
            println!("Coud not send Close due to {:?}, probably it is ok?", e);
        }
        n_msg
    });

    // Second task will receive messages from client and print in server console
    let mut recv_task = tokio::spawn(async move {
        let mut cnt = 0;
        while let Some(Ok(msg)) = receiver.next().await {
            cnt += 1;
            if process_msg(msg, who).is_break() {
                break;
            }
        }
        cnt
    });

    // If any one of the tasks exit, abort the other
    tokio::select! {
        rv_a = (&mut send_task) => {
            match rv_a {
                Ok(a) => println!("{:?} messages sent to {:?}", a, who),
                Err(e) => println!("Error sending messages {:?}", e)
            }
            recv_task.abort();
        }
        rv_b = (&mut recv_task) => {
            match rv_b {
                Ok(b) => println!("Received {:?} messages", b),
                Err(e) => println!("Error receiving messages {:?}", e)
            }
            send_task.abort();
        }
    }

    // Returning from the handler closes the websocket connection
    println!("Websocket context {:?} closed", who);
}

fn process_msg(msg: Message, who: SocketAddr) -> ControlFlow<(), ()> {
    match msg {
        Message::Text(t) => {
            println!(">>> {:?} sent str: {:?}", who, t);
        }
        Message::Binary(b) => {
            println!(">>> {:?} sent {} bytes: {:?}", who, b.len(), b);
        }
        Message::Close(c) => {
            if let Some(cf) = c {
                println!(
                    ">>> {:?} sent close with code {} and reason {}",
                    who, cf.code, cf.reason
                );
            } else {
                println!(
                    ">>> {:?} somehow sent close message without CloseFrame",
                    who
                );
            }
        }
        Message::Pong(v) => {
            println!(">>> {:?} send pong with {:?}", who, v);
        }
        Message::Ping(v) => {
            // You should not handle ping manually but let's just inspect its message here for now
            println!(">>> {:?} sent ping with {:?}", who, v);
        }
    }
    ControlFlow::Continue(())
}
