FROM rust:1-bookworm AS builder
WORKDIR /workspace
COPY rust-toolchain.toml ./
RUN cargo install cargo-about --version 0.9.0 --locked --features cli

COPY Cargo.toml Cargo.lock ./
COPY apps/bot apps/bot
COPY crates/bot-macros crates/bot-macros
RUN cargo build --release --locked --package bot
RUN cargo about generate --manifest-path apps/bot/Cargo.toml --config apps/bot/about.toml apps/bot/about.hbs --locked --fail -o /workspace/THIRD_PARTY_LICENSES

FROM gcr.io/distroless/cc-debian12:nonroot
WORKDIR /app
COPY --from=builder /workspace/target/release/bot /app/bot
COPY LICENSE /app/LICENSE
COPY --from=builder /workspace/THIRD_PARTY_LICENSES /app/THIRD_PARTY_LICENSES
ENTRYPOINT ["/app/bot"]
