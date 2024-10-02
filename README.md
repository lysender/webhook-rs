# webhook-rs: Your app's back door

Exposes your private endpoints to the world.

## Overview

- Exposes a local endpoint to the world
- Needs a cloud server to run the server version of the app
- Supports two modes:
    - Webhook mode - exposes an endpoint path
    - Website mode - exposes the root path

## Motivation

I needed a tool to expose a Github webhook endpoint for local
development purposes. Some paid solutions already exist but they are
not really worth it since I just need to run the tool once in a while.

I wrote this tool for a very simple use case:
- Run the server version of the app on a cloud server
- Run the client version of the app locally
- Stop the apps when done

## Server

Server usage:

```bash
# Dev mode
cargo run -- -c config-server.toml server

# Prod mode
webhook-rs -c config-server.toml server
```

Configuration (webhook mode):

```toml
web_port = 9000
tunnel_port = 9001
webhook_path = "/webhook"
jwt_secret = "super-strong-secret"
```

Website mode:

```toml
web_port = 9000
tunnel_port = 9001
webhook_path = "*"
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
target_address = "127.0.0.1:3000"
target_secure = false
jwt_secret = "super-strong-secret"
```

## Security

The proxy/tunnel client/servier communication is using basic HTTP/1.1 message
exchange over TCP. Authorization token may be intercepted over the network
traffic.

It is advised to only run the server application when needed to avoid security risks.

## Performance

Initial benchmarks show that the server can only handle 500+ RPS locally.
This is below expectations but it is enough for my use case.

I will be working on improving the performance in the future by
using some kind of connection pooling.

## Feedbacks

Feedbacks are welcome but please be gentle.
