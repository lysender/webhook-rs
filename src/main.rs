mod web;
mod workers;

use web::start_web_server;

fn main() {
    start_web_server();
}
