# Reduce Docker build memory footprint further (Rust + Vue)

## Context

The Coolify build (`docker-compose.coolify.yml` → root `Dockerfile`) already has substantial memory tuning documented in `CLAUDE.md` and inline Dockerfile comments: `CARGO_BUILD_JOBS=1`, `lld` linker, `opt-level = "s"`, `lto = false`, `strip = true`, sequential build-stage ordering, and `NODE_OPTIONS=--max-old-space-size=2048` for the Vue build. Despite this, the build still crashes the server, so there is more headroom to cut. Investigation found several concrete, low-risk levers that haven't been applied yet, plus one structural gap: **no swapfile is actually provisioned anywhere** — `CLAUDE.md` and the Dockerfile assert a host swapfile is "mandatory," but nothing in the repo creates one, so if the target VPS doesn't have swap configured, no amount of in-container tuning will prevent an OOM crash.

This plan focuses on squeezing the in-place Docker build (per the user's choice), not moving the build off the VPS.

## Changes

### 1. `Cargo.toml` — force single-threaded codegen
Add `codegen-units = 1` to `[profile.release]`. `CARGO_BUILD_JOBS=1` only serializes *which crate* rustc compiles at a time — it does not stop a single rustc invocation from running multiple codegen-unit threads in parallel (release profile defaults to 16 codegen units), each holding its own LLVM context in memory. Forcing 1 codegen unit is the standard lever for memory-constrained rustc builds (Raspberry Pi/small-VPS guides) and directly caps that per-crate parallel memory spike, at the cost of slower builds — an acceptable tradeoff here. Update the existing profile comment block to explain this alongside the existing `opt-level`/`lto`/`strip` rationale.

### 2. `Dockerfile` — make the Node heap ceiling configurable
`NODE_OPTIONS=--max-old-space-size=2048` is currently hardcoded. Expose it as a `ARG NODE_MAX_OLD_SPACE_MB=2048` (with a compose-level build arg default) so it can be tuned to the actual VPS's available RAM+swap without editing the Dockerfile — useful since we don't know the exact ceiling that's currently getting OOM-killed. Wire the same pattern for `CARGO_BUILD_JOBS` (currently hardcoded `ENV CARGO_BUILD_JOBS=1`) so both memory knobs are adjustable via build args, and surface both as build `args:` (with safe defaults matching current values) in `docker-compose.coolify.yml` and `docker-compose.yml`.

### 3. `frontend/vite.config.ts` — only generate sourcemaps when they'll be used
`build.sourcemap: true` is unconditional, so every build (including ones without `BUILD_BUGSINK_URL`/`BUILD_BUGSINK_AUTH_TOKEN` set) pays the extra memory/CPU cost of sourcemap generation during minification, even though the sourcemaps are only consumed by the conditional `sentryVitePlugin` upload block right above it. Change `sourcemap: true` to reuse the same condition already used for `sentryVitePlugin` (`Boolean(process.env.BUILD_BUGSINK_URL && process.env.BUILD_BUGSINK_AUTH_TOKEN)`), so a build without upload credentials (which is presumably the common/only case on the constrained VPS unless Bugsink is wired up) skips sourcemap generation entirely.

### 4. Flag the missing swapfile provisioning (documentation only, no code change)
Add a short note to `CLAUDE.md`'s existing "Building on a <200MB RAM VPS" gotcha making explicit that swap is asserted but **not automated anywhere in this repo** — it must be verified/created manually on the actual Coolify host (e.g. `swapon --show`, and if empty, `fallocate`/`mkswap`/`swapon` + `/etc/fstab` entry sized to comfortably cover peak build RSS). This is a host-level action outside docker-compose's reach, so it can't be scripted into the compose file — call it out as a manual pre-requisite to check before/alongside the code changes above, since none of the in-container tuning matters if the host has no swap at all.

## Out of scope (considered, not doing)

- **Trimming `tokio`'s `full` feature** to an explicit feature list: grepped actual `tokio::*` usage across `crates/` — it already uses most of what `full` provides (`fs`, `io`, `net`, `process`, `runtime`, `select`, `spawn`, `sync`, `time`, `#[tokio::main]`/`#[tokio::test]` macros). Only `signal`, `io-std`, `parking_lot`, `test-util` would drop, for modest savings vs. real risk of missing a feature and breaking the build. Not worth it here.
- **`sqlx` macros feature**: already correctly omitted — grepped for `query!`/`query_as!` and found no usage, confirming compile-time macro expansion isn't happening and doesn't need addressing.
- **Splitting `cargo build -p t2t` into multiple invocations**: no memory benefit — cargo already builds the same dependency graph either way; splitting wouldn't lower peak RSS, just add redundant invocation overhead.
- **Moving the build off the VPS into CI**: explicitly deferred per user's choice to squeeze the in-place build first.

## Verification

- `cargo build --release -p t2t` locally (or via `docker build`) still succeeds after adding `codegen-units = 1`; confirm binary still runs (`./t2t --help` or start it against local Postgres per the CLAUDE.md "Running locally" steps).
- `cd frontend && npm run build` still produces `dist/` with no sourcemap files when `BUILD_BUGSINK_URL`/`BUILD_BUGSINK_AUTH_TOKEN` are unset, and still produces them (and uploads) when those env vars are set — spot check `ls frontend/dist/**/*.map`.
- `docker compose -f docker-compose.coolify.yml build` (or the plain `docker-compose.yml`) completes end-to-end locally with default build args, confirming the new `ARG`-based `NODE_MAX_OLD_SPACE_MB`/`CARGO_BUILD_JOBS` wiring doesn't regress the default behavior.
- Since the actual crash only reproduces on the real constrained VPS, the real confirmation is a subsequent Coolify deploy after these changes land — flag to the user that local `docker build` success doesn't guarantee the fix, only that nothing regressed.
