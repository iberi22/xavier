# syntax=docker/dockerfile:1
# Xavier enrolment for Anthropic's OSS Scanner.
# Build online here; the scanner audits the image with NO network.
# Contract: checkout lives at /src (`COPY . /src`).
FROM rust:1.94-bookworm
ENV DEBIAN_FRONTEND=noninteractive \
    CARGO_TERM_COLOR=never \
    RUST_BACKTRACE=1
# protobuf-compiler + libssl-dev + pkg-config mirror the root Dockerfile
# builder (tonic/prost, OpenSSL linkage); build-essential/clang/cmake cover
# rusqlite-bundled, sqlite-vec and other cc-built deps.
RUN apt-get update && apt-get install -y --no-install-recommends \
      ca-certificates git build-essential clang cmake pkg-config \
      protobuf-compiler libssl-dev \
    && rm -rf /var/lib/apt/lists/*

# Login shells (bash -l) reset PATH from /etc/profile; keep cargo/rustc reachable there too.
RUN printf 'export PATH=/usr/local/cargo/bin:$PATH\n' > /etc/profile.d/cargo.sh

# Scanner contract: the checkout lives inside the image, at /src.
COPY . /src
WORKDIR /src

# Fetch every crate up front so later builds work offline.
RUN cargo fetch --locked

# Debug build, default features (cli-interactive, notifications,
# self-manage, panel-ui flag, pageindex). Heavy optionals
# (local-gllm, telegram, mesh, dao-evm, enterprise) stay off.
# panel-ui TypeScript is out of scope (needs node/pnpm).
ENV RUSTFLAGS="-C force-frame-pointers=yes"
RUN cargo build --locked \
    && cargo test --locked --no-run

# Smoke-check a fast hermetic subset; failures do not abort the build.
RUN (timeout 900 cargo test --locked --lib rag::prompt -- --test-threads=2 \
      || echo "WARNING: xavier smoke tests reported failures")
