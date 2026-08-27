FROM rust:1-bookworm AS builder
WORKDIR /workspace
COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY apps/bot apps/bot
COPY crates/bot-macros crates/bot-macros
RUN cargo build --release --locked --package bot

FROM gcr.io/distroless/cc-debian12:nonroot
WORKDIR /app
COPY --from=builder /workspace/target/release/bot /app/bot
ENTRYPOINT ["/app/bot"]
