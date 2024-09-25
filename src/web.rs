use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt};
use tokio::sync::Mutex;
use tokio::{
    io::BufReader,
    net::{TcpListener, TcpStream},
};

use tracing::info;

use crate::{config::Config, Result};

pub async fn start_web_server(
    client: Arc<Mutex<Option<TcpStream>>>,
    config: &Config,
) -> Result<()> {
    info!("Starting web server...");
    let address = format!("0.0.0.0:{}", config.web_port);
    let listener = TcpListener::bind(address.as_str()).await.unwrap();

    info!("Webhook web server started at {}", address);

    loop {
        let client_copy = client.clone();
        let conn = listener.accept().await;
        match conn {
            Ok((stream, _)) => {
                tokio::spawn(async move {
                    handle_connection(client_copy, stream).await;
                });
            }
            Err(e) => {
                info!("Error accepting connection: {:?}", e);
                break;
            }
        }
    }

    Ok(())
}

async fn handle_connection(client: Arc<Mutex<Option<TcpStream>>>, mut stream: TcpStream) -> () {
    let reader = BufReader::new(&mut stream);
    let request_line = reader.lines().next_line().await.unwrap().unwrap();

    info!("{}", request_line);

    let (status_line, contents) = match request_line.as_str() {
        "GET / HTTP/1.1" => ("HTTP/1.1 200 OK", "OK"),
        "GET /webhook HTTP/1.1" => ("HTTP/1.1 200 OK", "OK"),
        "POST /webhook HTTP/1.1" => ("HTTP/1.1 200 OK", "OK"),
        _ => ("HTTP/1.1 404 NOT FOUND", "Not Found"),
    };

    if contents == "OK" {
        let mut client_stream = client.lock().await;
        if let Some(cs) = client_stream.as_mut() {
            if let Err(cs_err) = cs.write_all(b"YOU GOT MAIL\n").await {
                // Failed to write to client
                let status_line = "HTTP/1.1 503 Service Unavailable";
                let contents = format!("Service Unavailable\n{}\n", cs_err);
                let length = contents.len();

                let response =
                    format!("{status_line}\r\nContent-Length: {length}\r\n\r\n{contents}");

                stream.write_all(response.as_bytes()).await.unwrap();
                return ();
            } else {
                let status_line = "HTTP/1.1 200 OK";
                let contents = "OK";
                let length = contents.len();

                let response =
                    format!("{status_line}\r\nContent-Length: {length}\r\n\r\n{contents}");

                stream.write_all(response.as_bytes()).await.unwrap();
                return ();
            }
        } else {
            let status_line = "HTTP/1.1 503 Service Unavailable";
            let contents = "Service Unavailable";
            let length = contents.len();

            let response = format!("{status_line}\r\nContent-Length: {length}\r\n\r\n{contents}");

            stream.write_all(response.as_bytes()).await.unwrap();
            return ();
        }
    }

    let length = contents.len();

    let response = format!("{status_line}\r\nContent-Length: {length}\r\n\r\n{contents}");
    stream.write_all(response.as_bytes()).await.unwrap();
}
