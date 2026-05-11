# Tracebox

Tracebox is a Linux-native immutable execution evidence runtime.

It does **not** try to deterministically replay execution.  
It records deterministic evidence about executions so failed runs, retries, and workspace mutations can be inspected later.

```text
command
↓
execution
↓
evidence capture
↓
immutable trace bundle
↓
inspection / diff / lineage
```

## What Tracebox captures

For every run, Tracebox writes an append-only trace bundle under `.traces/`:

```text
.traces/
└── trc_<uuidv7>/
    ├── manifest.json
    ├── manifest.sha256
    ├── stdout.log
    └── stderr.log
```

The manifest records:

- command argv;
- cwd;
- timestamps;
- duration;
- exit code;
- stdout/stderr artifact paths;
- artifact SHA-256 hashes;
- git commit/branch before and after;
- workspace dirty state before and after;
- conservative before/after workspace mutation diff;
- allowlisted environment variables;
- optional parent trace ID for lineage.

## Install / run

```bash
cargo build
```

Run a command:

```bash
cargo run -- run -- cargo test
```

List traces:

```bash
cargo run -- list
```

Inspect a trace:

```bash
cargo run -- inspect trc_...
```

Compare two traces:

```bash
cargo run -- diff trc_a trc_b
```

Use a custom trace root:

```bash
cargo run -- --trace-root /tmp/traces run -- cargo test
```

Link a retry to a parent trace:

```bash
cargo run -- run --parent trc_old -- cargo test
```

## Design rules

Tracebox v0.1 deliberately avoids:

- PTY capture;
- async runtimes;
- ptrace/eBPF;
- containers;
- DB storage;
- GUI/TUI;
- AI summaries;
- full deterministic replay.

The initial product is the evidence contract, not a UI.

## Why stdout/stderr are streamed

Tracebox never uses `Command::output()` for command execution because that buffers all output in memory.

Instead, it uses:

```text
spawn()
+
stdout/stderr pipes
+
reader threads
+
stream-to-artifact files
```

This is compatible with large CI logs and future live streaming.

## Security note

Tracebox does **not** persist all environment variables.

Environment capture is allowlist-only to avoid leaking secrets such as API keys, cloud credentials, SSH material, and tokens.

## Future direction

This standalone repo is intentionally structured so the evidence layer can later be embedded into runtimes such as `codex-rs`.

The future integration shape is:

```text
agent/tool runtime
  emits lifecycle facts
Tracebox
  records immutable evidence
```

Tracebox should not replace rollout/session persistence. It should record execution evidence and link back to runtime-owned IDs.
