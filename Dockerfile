FROM rust:slim-bookworm AS builder

WORKDIR /gachix
COPY . /gachix

RUN apt-get update && apt-get install -y libssl-dev pkg-config
RUN cargo build --release

FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y ca-certificates libssl3 && rm -rf /var/lib/apt/lists/*

COPY --from=builder /gachix/target/release/gachix /usr/local/bin/gachix

CMD ["gachix", "serve"]
