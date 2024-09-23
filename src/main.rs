use std::{
    fs,
    io::{prelude::*, BufReader},
    net::{TcpListener, TcpStream},
};

fn main() {
    let port = "7878";
    let address = format!("127.0.0.1:{}", port);
    let listener = TcpListener::bind(address.as_str()).unwrap();

    for stream in listener.incoming() {
        let st = stream.unwrap();

        handle_connection(st);
    }
}

fn handle_connection(mut stream: TcpStream) {
    let reader = BufReader::new(&mut stream);
    let request: Vec<_> = reader
        .lines()
        .map(|result| result.unwrap())
        .take_while(|line| !line.is_empty())
        .collect();

    println!("Request: {request:#?}");

    let status_line = "HTTP/1.1 200 OK";
    let contents = fs::read_to_string("templates/hello.html").unwrap();
    let length = contents.len();

    let response = format!("{status_line}\r\nContent-Length: {length}\r\n\r\n{contents}");
    stream.write_all(response.as_bytes()).unwrap();
}
