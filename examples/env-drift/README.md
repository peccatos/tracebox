# Env Drift Demo

This tiny crate demonstrates local/CI drift driven by `TRACEBOX_MODE`.

Run it from the example directory with an installed Tracebox binary:

```bash
cd examples/env-drift
TRACEBOX_MODE=stable ../../target/debug/tracebox run -- cargo test
TRACEBOX_MODE=broken ../../target/debug/tracebox run -- cargo test

../../target/debug/tracebox list
../../target/debug/tracebox diff <passed-trace-id> <failed-trace-id>
../../target/debug/tracebox report <failed-trace-id>
```

From the repository root you can also use `cargo run` for the feature-gated TUI:

```bash
cargo run --features tui -- tui
```

If you built Tracebox with `--features tui`, the installed binary can also open the browser:

```bash
../../target/debug/tracebox tui
```

For the full walkthrough and the expected diagnosis, see [docs/demo.md](../../docs/demo.md).
