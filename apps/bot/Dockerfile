FROM --platform=$BUILDPLATFORM rust:1-bookworm AS chef
ARG CARGO_CHEF_VERSION=0.1.78
ARG CARGO_ABOUT_VERSION=0.9.0
ARG CARGO_ZIGBUILD_VERSION=0.23.0
ARG ZIG_VERSION=0.16.0

WORKDIR /workspace
COPY rust-toolchain.toml ./

RUN cargo install cargo-chef \
      --version "$CARGO_CHEF_VERSION" \
      --locked

RUN cargo install cargo-about \
      --version "$CARGO_ABOUT_VERSION" \
      --locked \
      --features cli

RUN cargo install cargo-zigbuild \
      --version "$CARGO_ZIGBUILD_VERSION" \
      --locked

FROM chef AS planner

COPY . .
RUN cargo chef prepare \
      --recipe-path /recipe.json && \
    mkdir /manifests && \
    find . -name Cargo.toml \
      -exec cp --parents --target-directory /manifests {} + && \
    cp Cargo.lock /manifests/

FROM chef AS builder
ARG TARGETARCH

RUN curl --fail --silent --show-error --location \
      "https://ziglang.org/download/$ZIG_VERSION/zig-x86_64-linux-$ZIG_VERSION.tar.xz" \
      --output /tmp/zig.tar.xz && \
    mkdir /opt/zig && \
    tar --extract --xz \
      --file /tmp/zig.tar.xz \
      --directory /opt/zig \
      --strip-components 1 && \
    rm /tmp/zig.tar.xz

ENV PATH="/opt/zig:$PATH"

RUN case "$TARGETARCH" in \
      amd64) rust_target=x86_64-unknown-linux-gnu ;; \
      arm64) rust_target=aarch64-unknown-linux-gnu ;; \
      *) echo "unsupported architecture: $TARGETARCH" >&2; exit 1 ;; \
    esac; \
    echo "$rust_target" > /tmp/rust-target; \
    rustup target add "$rust_target"

COPY --from=planner /recipe.json ./recipe.json

RUN rust_target="$(cat /tmp/rust-target)" && \
    cargo chef cook \
      --release \
      --package bot \
      --target "$rust_target" \
      --zigbuild \
      --recipe-path recipe.json

COPY --from=planner /manifests/ ./

RUN rust_target="$(cat /tmp/rust-target)" && \
    cargo zigbuild \
      --release \
      --locked \
      --package bot \
      --target "$rust_target"

COPY crates ./crates

RUN find crates -type f -exec touch {} + && \
    rust_target="$(cat /tmp/rust-target)" && \
    cargo zigbuild \
      --release \
      --locked \
      --package bot \
      --target "$rust_target"

COPY apps/bot ./apps/bot

RUN find apps/bot -type f -exec touch {} + && \
    rust_target="$(cat /tmp/rust-target)" && \
    cargo zigbuild \
      --release \
      --locked \
      --package bot \
      --target "$rust_target" && \
    install -Dm755 \
      "target/$rust_target/release/bot" \
      /workspace/out/bot

RUN cargo about generate \
      --manifest-path apps/bot/Cargo.toml \
      --config apps/bot/about.toml \
      apps/bot/about.hbs \
      --locked \
      --fail \
      -o /workspace/THIRD_PARTY_LICENSES

FROM gcr.io/distroless/cc-debian13:nonroot
WORKDIR /app
COPY --from=builder /workspace/out/bot /app/bot
COPY LICENSE /app/LICENSE
COPY --from=builder /workspace/THIRD_PARTY_LICENSES /app/THIRD_PARTY_LICENSES
ENTRYPOINT ["/app/bot"]
