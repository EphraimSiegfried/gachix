FROM rust:slim-bookworm AS builder

WORKDIR /gachix
COPY . /gachix

RUN apt update
RUN apt install -y libssl-dev pkg-config
RUN cargo build --release

FROM ubuntu:25.10

COPY --from=builder /gachix/target/release/gachix gachix

CMD ["./gachix", "serve"]
