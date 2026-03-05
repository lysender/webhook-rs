FROM ubuntu:24.04
WORKDIR /app

RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates libssl3 \
 && rm -rf /var/lib/apt/lists/*

COPY target/release/webhook-server /app/webhook-server
RUN chmod +x /app/webhook-server

EXPOSE 12000
ENTRYPOINT ["/app/webhook-server"]
