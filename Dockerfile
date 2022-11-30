FROM rust:alpine as chef
RUN apk add --no-cache musl-dev && cargo install cargo-chef --locked

WORKDIR /app

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder
COPY --from=planner /app/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json
COPY . .
RUN cargo build --release

FROM scratch AS runtime
WORKDIR /app
COPY --from=builder /app/target/release/traewelling-exporter /
ENTRYPOINT ["/traewelling-exporter"]
