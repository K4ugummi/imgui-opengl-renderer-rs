FROM rust:1.90

COPY ./ ./

RUN cargo build --release
