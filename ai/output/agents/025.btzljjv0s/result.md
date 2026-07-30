#6 [chef 1/4] FROM docker.io/library/rust:1.88-slim@sha256:38bc5a86d998772d4aec2348656ed21438d20fcdce2795b56ca434cf21430d89
#6 resolve docker.io/library/rust:1.88-slim@sha256:38bc5a86d998772d4aec2348656ed21438d20fcdce2795b56ca434cf21430d89 0.0s done
#6 DONE 0.0s

#7 [internal] load build context
#7 transferring context: 13.31kB done
#7 DONE 0.0s

#8 [chef 2/4] RUN --mount=type=cache,target=/var/cache/apt,sharing=locked     --mount=type=cache,target=/var/lib/apt,sharing=locked     apt-get update && apt-get install -y --no-install-recommends pkg-config libssl-dev lld
#8 CACHED

#9 [chef 3/4] WORKDIR /app
#9 CACHED

#10 [chef 4/4] RUN --mount=type=cache,target=/usr/local/cargo/registry     cargo install cargo-chef --version 0.1.77 --locked
#10 CACHED

#11 [planner 1/3] COPY Cargo.toml Cargo.lock ./
#11 CACHED

#12 [planner 2/3] COPY crates/ crates/
#12 DONE 0.1s

#13 [planner 3/3] RUN cargo chef prepare --recipe-path recipe.json
#13 DONE 0.7s

#14 [rust-builder 1/6] COPY --from=planner /app/recipe.json recipe.json
#14 CACHED

#15 [rust-builder 2/6] RUN --mount=type=cache,target=/usr/local/cargo/registry     --mount=type=cache,target=/usr/local/cargo/git     --mount=type=cache,target=/app/target     cargo chef cook --release --recipe-path recipe.json -p t2t
#15 CACHED

#16 [rust-builder 3/6] COPY Cargo.toml Cargo.lock ./
#16 CACHED

#17 [rust-builder 4/6] COPY crates/ crates/
#17 DONE 0.2s

#18 [rust-builder 5/6] COPY migrations/ migrations/
#18 DONE 0.2s

#19 [rust-builder 6/6] RUN --mount=type=cache,target=/usr/local/cargo/registry     --mount=type=cache,target=/usr/local/cargo/git     --mount=type=cache,target=/app/target     cargo build --release -p t2t &&     cp target/release/t2t /t2t
#19 1.343    Compiling t2t v0.1.0 (/app/crates/t2t)
#19 36.83     Finished `release` profile [optimized] target(s) in 36.27s
#19 DONE 37.4s

#20 exporting to image
#20 exporting layers
#20 exporting layers 1.2s done
#20 exporting manifest sha256:8a2540541fa03745064dde0f2bcf4d19997f0d41afd25238badc5e960fb5b1f5 0.0s done
#20 exporting config sha256:c277938fdb848091067131c41d8efd3013b5f3b3bd6cd103db1b557e6ce118dd 0.0s done
#20 exporting attestation manifest sha256:5053a406e58df90b447b5db7d811fa9612949ef86ccb3bad9eb094dc8b7ca2f6
#20 exporting attestation manifest sha256:5053a406e58df90b447b5db7d811fa9612949ef86ccb3bad9eb094dc8b7ca2f6 0.1s done
#20 exporting manifest list sha256:b58a3675934d61023aef5c775b6cd947ed607d7939883c8d71cb42ed2f81804c 0.0s done
#20 naming to docker.io/library/t2t-rust-builder-test3:latest done
#20 unpacking to docker.io/library/t2t-rust-builder-test3:latest
#20 unpacking to docker.io/library/t2t-rust-builder-test3:latest 0.3s done
#20 DONE 1.7s

View build details: docker-desktop://dashboard/build/default/default/zo49ldaa4zn8pwfmk0pqzxtrp

real	0m42,359s
user	0m0,358s
sys	0m0,221s
