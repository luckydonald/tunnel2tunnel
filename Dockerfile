# syntax=docker/dockerfile:1

# Stage 1: Build Rust binary
FROM rust:1.88-slim AS rust-builder
RUN --mount=type=cache,target=/var/cache/apt,sharing=locked \
    --mount=type=cache,target=/var/lib/apt,sharing=locked \
    apt-get update && apt-get install -y --no-install-recommends pkg-config libssl-dev lld
WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY crates/ crates/
COPY migrations/ migrations/
# -j 1 bounds rustc/LLVM to one crate at a time — on a <200MB box, parallel codegen units are
# what OOMs the build, not the binary itself. lld uses substantially less memory than GNU ld
# for the final link. A swapfile on the host is still required (see deploy notes) — this just
# keeps peak RSS as low as the toolchain allows so the swap doesn't have to absorb as much.
ENV CARGO_BUILD_JOBS=1
ENV RUSTFLAGS="-C link-arg=-fuse-ld=lld"
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    --mount=type=cache,target=/app/target \
    cargo build --release -p t2t && \
    cp target/release/t2t /t2t

# Stage 2: Build Vue frontend
FROM node:24-alpine AS node-builder
# Forces this stage to wait for rust-builder to finish before running npm ci/build below —
# without it BuildKit runs both heavy build stages concurrently, doubling peak RAM on a
# machine that barely fits one of them.
WORKDIR /app/frontend
COPY --from=rust-builder /t2t /tmp/.rust-build-complete
# V8 sizes its default heap ceiling off detected physical RAM, which on a <200MB box is low
# enough that the build hits a heap-OOM before ever touching the swap that's supposed to
# cover the shortfall. Setting this explicitly overrides that auto-detection.
ENV NODE_OPTIONS=--max-old-space-size=2048
# Release/build metadata baked into the bundle at build time (see frontend/vite.config.ts).
ARG SOURCE_COMMIT
ARG GIT_BRANCH
ARG BUILD_TIME
ARG VITE_SENTRY_DSN
ARG VITE_SENTRY_ENVIRONMENT
ARG VITE_SENTRY_RELEASE
ARG VITE_SENTRY_TRACES_SAMPLE_RATE
# Sourcemap upload secrets — deliberately not VITE_-prefixed so Vite doesn't inline them into the bundle.
ARG BUILD_BUGSINK_URL
ARG BUILD_BUGSINK_AUTH_TOKEN
ARG BUILD_BUGSINK_PROJECT_SLUG
COPY frontend/package.json frontend/package-lock.json* ./
RUN --mount=type=cache,target=/root/.npm \
    npm ci --prefer-offline
COPY frontend/ ./
RUN npm run build

# Stage 3: Runtime
FROM debian:bookworm-slim
RUN --mount=type=cache,target=/var/cache/apt,sharing=locked \
    --mount=type=cache,target=/var/lib/apt,sharing=locked \
    apt-get update && apt-get install -y --no-install-recommends ca-certificates
WORKDIR /app
COPY --from=rust-builder /t2t ./t2t
COPY --from=node-builder /app/frontend/dist ./frontend/dist/
ENV STATIC_DIR=/app/frontend/dist
EXPOSE 3000 2222
CMD ["./t2t"]
