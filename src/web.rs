use tokio::io::{AsyncBufReadExt, AsyncWriteExt};
use tokio::{
    io::BufReader,
    net::{TcpListener, TcpStream},
};

use tracing::info;

use crate::{config::Config, Result};

pub async fn start_web_server(config: &Config) -> Result<()> {
    info!("Starting web server...");
    let address = format!("0.0.0.0:{}", config.web_port);
    let listener = TcpListener::bind(address.as_str()).await.unwrap();

    info!("Webhook web server started at {}", address);

    loop {
        let conn = listener.accept().await;
        match conn {
            Ok((stream, _)) => {
                tokio::spawn(async {
                    handle_connection(stream).await;
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

async fn handle_connection(mut stream: TcpStream) {
    let reader = BufReader::new(&mut stream);
    let request_line = reader.lines().next_line().await.unwrap().unwrap();

    info!("{}", request_line);

    let (status_line, contents) = match request_line.as_str() {
        "GET / HTTP/1.1" => ("HTTP/1.1 200 OK", "OK"),
        "GET /webhook HTTP/1.1" => ("HTTP/1.1 200 OK", "OK"),
        "POST /webhook HTTP/1.1" => ("HTTP/1.1 200 OK", "OK"),
        _ => ("HTTP/1.1 404 NOT FOUND", "Not Found"),
    };

    let length = contents.len();

    let response = format!("{status_line}\r\nContent-Length: {length}\r\n\r\n{contents}");
    stream.write_all(response.as_bytes()).await.unwrap();
}
