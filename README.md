# webhook-rs: Exposes your localhost webhooks to the world

## Overview

I just needed a way to expose my Github webhooks to the internet
while developing applications locally.

There are already some great tools like `ngrok` but they are paid solutions.
Using SSH proxy might actually work but I don't like a complicated setup
since I'm on Windows. I need something that I can run anytime and stop anytime too.

Since I already have my own cloud server, I decided to just develop a simple
proxy/tunnel (or whatever you call it) that forwards requests to my local machine
and send the response back.

## Server

Server usage:

```bash
# Dev mode
cargo run -- -c config-server.toml server

# Prod mode
webhook-rs -c config-server.toml server
```

Configuration:

```toml
web_port = 9000
tunnel_port = 9001
webhook_path = "/webhook"
jwt_secret = "super-strong-secret"
```

## Client

Client usage:

```bash
# Dev mode
cargo run -- -c config-client.toml client

# Prod mode
webhook-rs -c config-client.toml client
```

Configuration:

```toml
tunnel_address = "127.0.0.1:9001"
target_url = "http://127.0.0.1:3000"
jwt_secret = "super-strong-secret"
```
