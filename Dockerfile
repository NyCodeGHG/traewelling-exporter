FROM rustlang/rust:nightly-alpine as chef
RUN apk add --no-cache musl-dev
RUN cargo install cargo-chef
WORKDIR /app

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder
COPY --from=planner /app/rust-toolchain.toml rust-toolchain.toml
COPY --from=planner /app/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json
COPY . .
RUN cargo build --release

FROM scratch AS runtime
WORKDIR /app
EXPOSE 3000
COPY --from=builder /app/target/release/traewelling-exporter /
ENTRYPOINT ["/traewelling-exporter"]
