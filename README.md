# webhook-rs: Your app's back door

Exposes your private endpoints to the world.

## Overview

- Exposes a local endpoint to the internet
- Requires a server sitting in the cloud and a client running locally
- Supports two modes:
    - Webhook mode - exposes an endpoint path
    - Website mode - exposes the root path

## Motivation

I needed a tool to expose a Github webhook endpoint for local
development purposes. Some paid solutions already exist but they are
not really worth it since I just need to run the tool once in a while.

I wrote this tool for a very simple use case:
- Run the server app on a cloud server to accept webhook requests
- Run the client app to forward requests to the target app running locally
- Stop the apps when done

## Just give me the EXE!

You need to compile the app using Rust's development tools.

See: https://rustup.rs

```bash
cargo build --release
```

## Server

You need to either build the executable image on the cloud server or
build it locally and upload to the server.

If you choose to build locally, make sure your OS is the same with the server.

```bash
/path/to/webhook-rs -c config-server.toml server
```

Configuration (webhook mode):

```toml
web_address = "127.0.0.1:9000"
tunnel_port = 9001
webhook_path = "/webhook"
jwt_secret = "super-strong-secret"
```

Website mode:

```toml
web_address = "127.0.0.1:9000"
tunnel_port = 9001
webhook_path = "*"
jwt_secret = "super-strong-secret"
```

## Client

Run the app as a client.

```bash
/path/to/webhook-rs -c config-client.toml client
```

Configuration:

```toml
tunnel_address = "127.0.0.1:9001"
target_address = "127.0.0.1:4200"
target_secure = false
jwt_secret = "super-strong-secret"
```

## What's the target?

The target is your actual application that has a webhook endpoint. An example would be
a Github app that listens to webhook events.

If you just need a dummy target application, you can build a simple HTTP server that responds
with 200 OK to all requests.

If you need a HTTP server that simply reponds with 200 OK or echoes back the request body, checkout `ok-rs`:

https://github.com/lysender/ok-rs

## Security

The proxy client/server communication is using basic HTTP/1.1 message
exchange over TCP without any encryption.

Use it at your own risk.

## Performance

Although the tool is designed for testing applications for development purposes,
it actually has good enough performance to be used in production.

## Feedbacks

Feedbacks, issues, and PRs are welcome.
