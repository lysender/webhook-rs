use std::{
    io::{prelude::*, BufReader},
    net::{TcpListener, TcpStream},
};

use tracing::info;

use crate::{config::ServerConfig, workers::ThreadPool};

pub fn start_web_server(config: &ServerConfig) {
    let address = format!("0.0.0.0:{}", config.web_port);
    let listener = TcpListener::bind(address.as_str()).unwrap();

    info!("Webhook web server started at {}", address);

    let pool = ThreadPool::new(4);

    for stream in listener.incoming() {
        let st = stream.unwrap();

        pool.execute(|| {
            handle_connection(st);
        });
    }
}

fn handle_connection(mut stream: TcpStream) {
    let reader = BufReader::new(&mut stream);
    let request_line = reader.lines().next().unwrap().unwrap();

    println!("{}", request_line);

    let (status_line, contents) = match request_line.as_str() {
        "GET / HTTP/1.1" => ("HTTP/1.1 200 OK", "OK"),
        "GET /webhook HTTP/1.1" => ("HTTP/1.1 200 OK", "OK"),
        "POST /webhook HTTP/1.1" => ("HTTP/1.1 200 OK", "OK"),
        _ => ("HTTP/1.1 404 NOT FOUND", "Not Found"),
    };

    let length = contents.len();

    let response = format!("{status_line}\r\nContent-Length: {length}\r\n\r\n{contents}");
    stream.write_all(response.as_bytes()).unwrap();
}
