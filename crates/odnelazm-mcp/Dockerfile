FROM rust:1.93 AS builder
WORKDIR /app
COPY . .
RUN cargo build --release --bin odnelazm-mcp-web

FROM ubuntu:24.04

RUN apt-get update && apt-get install -y \
  ca-certificates \
  && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/odnelazm-mcp-web /usr/local/bin/odnelazm-mcp-web

WORKDIR /app

ARG HOST="127.0.0.1"
ARG PORT="8055"

ENV BIND_ADDRESS="${HOST}:${PORT}"
ENV PORT=${PORT}

EXPOSE ${PORT}

CMD ["odnelazm-mcp-web"]
