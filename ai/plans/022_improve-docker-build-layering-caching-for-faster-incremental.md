# Improve Docker build layering/caching for faster incremental builds

## Context

The memory-reduction pass (already merged) made the Coolify Docker build survive on a constrained VPS, but every build still recompiles more than necessary because the Rust stage has no separation between "compile external dependencies" and "compile our own crates." Right now `Dockerfile`'s `rust-builder` stage does `COPY Cargo.toml Cargo.lock ./` → `COPY crates/ crates/` → `COPY migrations/ migrations/` → one `cargo build --release -p t2t`. Any source change (the common case — editing a `.rs` file) invalidates the Docker layer that the `cargo build` RUN lives on, so Docker always re-executes that RUN step; it only stays fast today because of the `--mount=type=cache,target=/app/target` cache mount, which is BuildKit-local and isn't portable across a fresh builder, a pruned cache, or an exported/imported registry cache. The user wants a real dependency/source split — via cargo-chef for Rust — plus wants npm's existing split reviewed as a reference pattern.

The Vue stage (`node-builder`) is **already** correctly split: `COPY package.json package-lock.json* ./` → `npm ci` (its own cache-mount-backed layer) → `COPY frontend/ ./` → `npm run build`. Source changes only invalidate the build step, never the `npm ci` step. No structural change needed there — it's the model the Rust stage should match.

## Changes

### `Dockerfile` — split the `rust-builder` stage into `chef` / `planner` / `rust-builder` using cargo-chef

cargo-chef is the standard tool for this exact problem in multi-crate Rust workspaces (handles the workspace's internal path dependencies between `tunnel2tunnel-core`/`-web`/`-ssh`/`t2t` correctly, which a hand-rolled dummy-`main.rs` stub approach would not do cleanly for 5 interdependent crates).

1. **`chef` stage** (`FROM rust:1.88-slim AS chef`): keep the existing `apt-get install pkg-config libssl-dev lld` here, then `RUN cargo install cargo-chef --locked` (pin an explicit version rather than latest, resolved at implementation time) using the same `--mount=type=cache,target=/usr/local/cargo/registry` pattern already used elsewhere. This layer only changes when the base image or the pinned cargo-chef version changes — effectively cached forever.

2. **`planner` stage** (`FROM chef AS planner`): `COPY Cargo.toml Cargo.lock ./` + `COPY crates/ crates/` (no `migrations/` — not needed for dependency planning), then `RUN cargo chef prepare --recipe-path recipe.json -p t2t`. This is fast (just parses manifests) and reruns on every source change, but its *output* (`recipe.json`) only changes when `Cargo.toml`/`Cargo.lock` actually change.

3. **`rust-builder` stage** (`FROM chef AS rust-builder`): 
   - `COPY --from=planner /app/recipe.json recipe.json`
   - Same `ARG CARGO_BUILD_JOBS=1` / `ENV RUSTFLAGS="-C link-arg=-fuse-ld=lld"` as today, set before the cook step so cached dep artifacts and the final build use identical flags.
   - `RUN --mount=type=cache,target=/usr/local/cargo/registry --mount=type=cache,target=/usr/local/cargo/git --mount=type=cache,target=/app/target cargo chef cook --release --recipe-path recipe.json -p t2t` — compiles **only** external dependencies. This Docker layer is keyed on `recipe.json`'s content, i.e. decoupled from `crates/`/`migrations/` changes entirely — a genuine portable layer, not just a local cache-mount artifact.
   - Then the existing `COPY Cargo.toml Cargo.lock ./` / `COPY crates/ crates/` / `COPY migrations/ migrations/` (migrations only needed here — confirmed via `sqlx::migrate!("../../migrations")` in `crates/t2t/src/main.rs`, embedded at compile time, not read at runtime) followed by the existing `cargo build --release -p t2t && cp target/release/t2t /t2t`, using the same cache mounts. This step now only compiles the workspace's own crates, since dependency artifacts are already resident in the shared `target`/`registry` cache mounts from the cook step.

Net effect: editing application source no longer forces re-resolution/recompilation of the dependency graph, and the separation holds even if the BuildKit cache mount is ever pruned or the build runs on a fresh runner (unlike today's setup, which only stays fast because of the mount cache persisting).

### Everything else stays as-is
- `node-builder` stage: no changes — already correctly split.
- Runtime stage, `docker-compose.yml`/`docker-compose.coolify.yml`, memory-tuning ARGs (`CARGO_BUILD_JOBS`, `NODE_MAX_OLD_SPACE_MB`) from the prior task: unchanged, just carried through to the new `rust-builder` stage.
- `COPY --from=rust-builder /t2t /tmp/.rust-build-complete` in `node-builder` (forces sequential stage ordering to bound peak memory) and `COPY --from=rust-builder /t2t ./t2t` in the runtime stage: unchanged, `rust-builder` still produces `/t2t` as the final artifact.

## Out of scope
- Restructuring the npm/Vue stage — already follows the correct pattern.
- Reordering `COPY migrations/` relative to `COPY crates/` in the final build step — irrelevant once cargo-chef isolates the dependency layer; migrations correctly only affect the final (already-source-dependent) build layer since they're embedded via `sqlx::migrate!()` at compile time.
- Adding a `--mount=type=cache` for `frontend/node_modules` — would prevent `node_modules` from persisting into the layer that `npm run build` needs; the current pattern (cache mount only for `/root/.npm`, real `node_modules` written to a normal layer) is already correct.

## Verification
- `docker build --target rust-builder .` (or full `docker build .`) succeeds and produces a working `/t2t` binary, same as before.
- Confirm the caching win: run the build twice with only a `.rs` file touched between runs (no `Cargo.toml`/`Cargo.lock` change) and confirm via `docker build --progress=plain` output that the `cargo chef cook` layer/step is skipped or cache-hit on the second run, while the final `cargo build -p t2t` step re-runs.
- `docker compose -f docker-compose.yml config` / `-f docker-compose.coolify.yml config` still resolve cleanly (no compose-file changes expected, but confirms nothing broke).
- Re-run `cargo check --release -p t2t` locally as a sanity check that `-p t2t` scoping still matches what `cargo chef cook -p t2t` will build.
