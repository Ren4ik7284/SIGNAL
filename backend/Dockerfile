FROM rust:trixie AS builder

WORKDIR /app
COPY . .

RUN cargo build --release

FROM debian:trixie-slim

WORKDIR /app

COPY --from=builder /app/target/release/signal-audio-backend /app/

EXPOSE 8085

CMD ["./signal-audio-backend"]
