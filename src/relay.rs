use std::{
    io::{prelude::*, BufRead, BufReader},
    net::{TcpListener, TcpStream},
};

use crate::workers::ThreadPool;

pub fn start_relay_server() {
    let port = "9001";
    let address = format!("127.0.0.1:{}", port);
    let listener = TcpListener::bind(address.as_str()).unwrap();
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
