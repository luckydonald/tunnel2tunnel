# Stage 1: Build Rust binary
FROM rust:1.87-slim AS rust-builder
RUN apt-get update && apt-get install -y --no-install-recommends pkg-config libssl-dev && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY crates/ crates/
COPY migrations/ migrations/
RUN cargo build --release -p t2t

# Stage 2: Build Vue frontend
FROM node:24-alpine AS node-builder
WORKDIR /app/frontend
COPY frontend/package.json ./
RUN npm install
COPY frontend/ ./
RUN npm run build

# Stage 3: Runtime
FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY --from=rust-builder /app/target/release/t2t ./
COPY --from=node-builder /app/frontend/dist ./frontend/dist/
ENV STATIC_DIR=/app/frontend/dist
EXPOSE 3000 2222
CMD ["./t2t"]
