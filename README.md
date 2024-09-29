# webhook-rs: Your app's back door

Exposes your private endpoints to the world.

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
