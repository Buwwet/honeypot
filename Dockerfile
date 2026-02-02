FROM rust:1.85 AS chef 
RUN cargo install cargo-chef 
WORKDIR /app

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder
COPY --from=planner /app/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json
COPY . .
RUN cargo build --release

FROM debian:bookworm-slim AS runtime
# For Reqwest to work?
RUN apt-get update && apt-get install -y ca-certificates
WORKDIR /app
COPY --from=builder /app/target/release/honeypot /app/honeypot
ENTRYPOINT ["/app/honeypot"]