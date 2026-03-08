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
- Run the client app which connects to the server using websocket
- The server forwards webhook requests to the client via websocket
- The client app to forward requests to the target app running locally
- App response will be sent back to the server and back to the calling third party service
- Stop the apps when done

## Just give me the EXE!

You need to compile the app using Rust's development tools.

See: https://rustup.rs

Building the server:

```bash
cargo build -p webhook-server --release
```

Building the client:

```bash
cargo build -p webhook-client --release
```

## Server

You need to either build the executable image on the cloud server or
build it locally and upload to the server.

If you choose to build locally, make sure your OS is the same with the server.

```bash
export WEBHOOK_SERVER_WEB_ADDRESS="127.0.0.1:14000"
export WEBHOOK_SERVER_WEBHOOK_PATH="/webhooks"
export WEBHOOK_SERVER_JWT_SECRET="super-strong-secret"
/path/to/webhook-server
```

See `.env.example-server` for a starter file.

Configuration (webhook mode):

```env
WEBHOOK_SERVER_WEB_ADDRESS=127.0.0.1:14000
WEBHOOK_SERVER_WEBHOOK_PATH=/webhooks
WEBHOOK_SERVER_JWT_SECRET=super-strong-secret
```

Website mode:

```env
WEBHOOK_SERVER_WEB_ADDRESS=127.0.0.1:14000
WEBHOOK_SERVER_WEBHOOK_PATH=*
WEBHOOK_SERVER_JWT_SECRET=super-strong-secret
```

## Client

Run the app as a client.

```bash
/path/to/webhook-client --config config-client.toml
```

Configuration:

```toml
server_address = "ws://127.0.0.1:14000/_ws"
targets = [
    { host = "127.0.0.1:3001", secure = false, source_path = "/webhooks/slackWebhooks", dest_path = "/slackWebhooks" },
    { host = "127.0.0.1:3002", secure = false, source_path = "/webhooks/githubWebhook", dest_path = "/githubWebhook" },

    # Catch all routes
    { host = "127.0.0.1:4200", secure = false, source_path = "/", dest_path = "/" },
]
jwt_secret = "super-strong-secret"
```

## Docker

Webhook server supports Docker deployment:
- Either build the webhook-server binary locally and upload to the server or
- Build the binary on the server
- Build the Docker image and containers based on the example docker-compose.yml

To build the binary on the server (not advisable):

```bash
cd /path/to/webhook-rs;
cargo build -p webhook-server --release;
```

Better option is to build the binary locally and upload to the server on the same path:

```bash
target/release/webhook-server
```

Create a `docker-compose.yml` based on the example given, then run docker-compose.

```bash
cd /path/to/webhook-rs;
docker compose up -d
```

## What's the target?

The target is your actual application that has a webhook endpoint. An example would be
a Github app that listens to webhook events.

If you just need a dummy target application, you can build a simple HTTP server that responds
with 200 OK to all requests.

If you need a HTTP server that simply reponds with 200 OK or echoes back the request body, checkout `ok-rs`:

https://github.com/lysender/ok-rs

## Security

Use HTTPS for the exposed web server to ensure secure connection. Both the webhook requests
from third party services and the client-server communication will use the same secure connection.

## Performance

Although the tool is designed for testing applications for development purposes,
it actually has good enough performance to be used in production.

## Feedbacks

Feedbacks, issues, and PRs are welcome.

