The SSH-layer tracking and web routes are implemented and the workspace builds cleanly (`cargo build --workspace --tests`). I'm now waiting for `cargo test --workspace` (running in the background against a local Postgres instance) to confirm the e2e test's new `ActiveTunnels` assertions pass before committing.

I'll report back once the test run finishes.