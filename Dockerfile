FROM rust:1.92-slim-bookworm AS builder
LABEL authors="dcadea"

WORKDIR /usr/src/that-limit
COPY . .
RUN cargo build --release --features http

FROM debian:bookworm-slim
LABEL authors="dcadea"
RUN apt-get update && apt-get install -y openssl ca-certificates && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY --from=builder /usr/src/that-limit/target/release/that-limit /app/
COPY ./static/ /app/static/

EXPOSE 8000

CMD ["./that-limit"]
