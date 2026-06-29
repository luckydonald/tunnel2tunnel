# syntax=docker/dockerfile:1

# Stage 1: Build Rust binary
FROM rust:1.88-slim AS rust-builder
RUN --mount=type=cache,target=/var/cache/apt,sharing=locked \
    --mount=type=cache,target=/var/lib/apt,sharing=locked \
    apt-get update && apt-get install -y --no-install-recommends pkg-config libssl-dev
WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY crates/ crates/
COPY migrations/ migrations/
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    --mount=type=cache,target=/app/target \
    cargo build --release -p t2t && \
    cp target/release/t2t /t2t

# Stage 2: Build Vue frontend
FROM node:24-alpine AS node-builder
WORKDIR /app/frontend
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
