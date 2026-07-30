…
 Compiling sentry-tower v0.48.3
#15 539.4    Compiling num-bigint v0.4.6
#15 542.2    Compiling tower-sessions-memory-store v0.14.0
#15 542.9    Compiling enum_dispatch v0.3.13
#15 544.0    Compiling russh-util v0.52.0
#15 544.2    Compiling delegate v0.13.5
#15 545.7    Compiling data-encoding v2.11.0
#15 548.1    Compiling lazy_static v1.5.0
#15 548.1    Compiling md5 v0.8.0
#15 548.4    Compiling hex-literal v1.1.0
#15 548.5    Compiling http-range-header v0.4.2
#15 549.0    Compiling tower-http v0.7.0
#15 551.0    Compiling russh v0.61.2
#15 564.8    Compiling sharded-slab v0.1.7
#15 565.6    Compiling tower-sessions v0.14.0
#15 570.6    Compiling tower-sessions-sqlx-store v0.15.0
#15 575.2    Compiling matchers v0.2.0
#15 575.2    Compiling tracing-log v0.2.0
#15 575.5    Compiling thread_local v1.1.9
#15 575.7    Compiling nu-ansi-term v0.50.3
#15 576.4    Compiling tracing-subscriber v0.3.23
#15 580.7    Compiling tunnel2tunnel-web v0.0.1 (/app/crates/tunnel2tunnel-web)
#15 580.7    Compiling tunnel2tunnel-ssh v0.0.1 (/app/crates/tunnel2tunnel-ssh)
#15 580.8    Compiling t2t v0.0.1 (/app/crates/t2t)
#15 581.1     Finished `release` profile [optimized] target(s) in 9m 40s
#15 DONE 581.6s

#16 [rust-builder 3/6] COPY Cargo.toml Cargo.lock ./
#16 DONE 0.2s

#17 [rust-builder 4/6] COPY crates/ crates/
#17 DONE 0.2s

#18 [rust-builder 5/6] COPY migrations/ migrations/
#18 DONE 0.2s

#19 [rust-builder 6/6] RUN --mount=type=cache,target=/usr/local/cargo/registry     --mount=type=cache,target=/usr/local/cargo/git     --mount=type=cache,target=/app/target     cargo build --release -p t2t &&     cp target/release/t2t /t2t
#19 1.873    Compiling tunnel2tunnel-core v0.1.0 (/app/crates/tunnel2tunnel-core)
#19 3.392    Compiling tunnel2tunnel-web v0.1.0 (/app/crates/tunnel2tunnel-web)
#19 5.578    Compiling tunnel2tunnel-ssh v0.1.0 (/app/crates/tunnel2tunnel-ssh)
#19 9.591    Compiling t2t v0.1.0 (/app/crates/t2t)
#19 44.87     Finished `release` profile [optimized] target(s) in 44.20s
#19 DONE 45.1s

#20 exporting to image
#20 exporting layers
#20 exporting layers 14.5s done
#20 exporting manifest sha256:de476b16109a52af0da526e522444ed85295d9aa00a870ed1936797c011f3453 0.0s done
#20 exporting config sha256:3cb30d68bd9ee2cf96341c623b0650442a1d78e00e5b906ded367064a072d470 0.0s done
#20 exporting attestation manifest sha256:7bf56da56921cb26d0d25236eb1b26d68e4bb396a878d3d1672375647e4a0a49 0.1s done
#20 exporting manifest list sha256:b084ca046e1bb058eb1cd264d9df6309f713cb7047fbfffa4da5e44ea4605ee9
#20 exporting manifest list sha256:b084ca046e1bb058eb1cd264d9df6309f713cb7047fbfffa4da5e44ea4605ee9 0.0s done
#20 naming to docker.io/library/t2t-rust-builder-test:latest done
#20 unpacking to docker.io/library/t2t-rust-builder-test:latest
#20 unpacking to docker.io/library/t2t-rust-builder-test:latest 1.9s done
#20 DONE 16.7s

View build details: docker-desktop://dashboard/build/default/default/s894diamywbyrvkypw7x7l4my

real	10m47,169s
user	0m1,062s
sys	0m0,704s
