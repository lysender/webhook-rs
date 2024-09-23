use std::{
    fs,
    io::{prelude::*, BufReader},
    net::{TcpListener, TcpStream},
};

mod workers;

use workers::ThreadPool;

fn main() {
    let port = "7878";
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

    let (status_line, filename) = match request_line.as_str() {
        "GET / HTTP/1.1" => ("HTTP/1.1 200 OK", "templates/hello.html"),
        _ => ("HTTP/1.1 404 NOT FOUND", "templates/404.html"),
    };

    let contents = fs::read_to_string(filename).unwrap();
    let length = contents.len();

    let response = format!("{status_line}\r\nContent-Length: {length}\r\n\r\n{contents}");
    stream.write_all(response.as_bytes()).unwrap();
}
