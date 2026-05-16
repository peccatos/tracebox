# Tracebox

Tracebox captures execution evidence for commands and helps diagnose CI/local drift, flaky tests, and environment-dependent failures.

It records what happened around a command so you can compare runs instead of guessing why behavior changed.

---

## Problem

Tracebox helps answer questions like:

- Why did this pass locally but fail in CI?
- Why did this flaky test fail this time?
- What changed between two command executions?
- Which env/git/workspace/artifact differences affected the run?

---

## Quickstart

```bash
cargo build
cargo run -- run -- echo hello
cargo run -- list
cargo run -- inspect <trace-id>
cargo run -- report <trace-id>
```

Tracebox preserves the wrapped command exit code and writes an immutable trace bundle under `.traces/`.

---

## Core Commands

- `run` captures a command and its evidence.
- `list` shows stored traces.
- `inspect` prints a trace summary and evidence tails.
- `verify` checks recorded artifact integrity.
- `validate` checks trace structure and semantic consistency.
- `diff` compares two traces.
- `report` writes a Markdown report for a trace.
- `archive` moves a trace into the archived area.
- `restore` moves an archived trace back to active storage.
- `tui` opens the feature-gated browser with `cargo run --features tui -- tui`.

---

## Env Drift Demo

See [examples/env-drift/](examples/env-drift/) and [docs/demo.md](docs/demo.md).

```bash
cd examples/env-drift
TRACEBOX_MODE=stable ../../target/debug/tracebox run -- cargo test
TRACEBOX_MODE=broken ../../target/debug/tracebox run -- cargo test

../../target/debug/tracebox diff <passed-trace-id> <failed-trace-id>
../../target/debug/tracebox report <failed-trace-id>
```

Expected result:

- stable run passes
- broken run fails
- diff shows `TRACEBOX_MODE: stable -> broken`
- report preserves stdout/stderr/manifest evidence

---

## TUI

```bash
cargo run --features tui -- tui
```

The TUI is optional behind the `tui` feature. It provides active and archived views, filtering, report/verify/archive/restore actions, and does not execute commands.

---

## CI Artifacts

```yaml
- name: Test with Tracebox
  run: tracebox run -- cargo test

- name: Upload Tracebox evidence
  if: always()
  uses: actions/upload-artifact@v4
  with:
    name: tracebox-evidence
    path: .traces/
```

---

## Safety / Non-Goals

Tracebox v0.1 alpha does not implement:

- syscall replay;
- ptrace engine;
- eBPF tracing;
- sandboxing;
- distributed tracing;
- AI diagnosis;
- process isolation;
- network capture.

---

## Alpha Status

Tracebox is currently alpha software.
The trace format and CLI may still change.
Use it for local/dev/CI evidence capture, not as a security boundary.

---

## Install from GitHub Release

Linux x86_64 GNU builds are published as release artifacts.

Example using `v0.1.0-alpha.5`:

```bash
mkdir -p /tmp/tracebox-install
cd /tmp/tracebox-install

curl -L -O \
  https://github.com/peccatos/tracebox/releases/download/v0.1.0-alpha.5/tracebox-v0.1.0-alpha.5-x86_64-unknown-linux-gnu.tar.gz

curl -L -O \
  https://github.com/peccatos/tracebox/releases/download/v0.1.0-alpha.5/checksums.txt

sha256sum -c checksums.txt

tar -xzf tracebox-v0.1.0-alpha.5-x86_64-unknown-linux-gnu.tar.gz

sudo install -m 0755 \
  tracebox-v0.1.0-alpha.5-x86_64-unknown-linux-gnu/tracebox \
  /usr/local/bin/tracebox

tracebox --version
```

Expected:

```text
tracebox 0.1.0
```

---

## Build from source

```bash
git clone https://github.com/peccatos/tracebox.git
cd tracebox

cargo build --release
```

Run local checks:

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --locked
```

---

## Inspect a trace

```bash
tracebox inspect <trace-id>
```

Show stdout and stderr tails:

```bash
tracebox inspect <trace-id> --stdout --stderr
```

Machine-readable output:

```bash
tracebox inspect <trace-id> --json
```

---

## List traces

```bash
tracebox list
```

JSON output:

```bash
tracebox list --json
```

---

## Compare traces

```bash
tracebox diff <trace-a> <trace-b>
```

JSON output:

```bash
tracebox diff <trace-a> <trace-b> --json
```

The diff command compares:

- exit code;
- duration;
- command;
- cwd;
- git branch and commit;
- dirty state;
- workspace changes;
- stdout/stderr artifact hashes.

---

## Verify trace integrity

```bash
tracebox verify <trace-id>
```

JSON output:

```bash
tracebox verify <trace-id> --json
```

`verify` recomputes hashes from artifacts on disk and compares them with the
recorded evidence.

It checks:

- `manifest.json`;
- `manifest.sha256`;
- `stdout.log`;
- `stderr.log`;
- optional PTY artifacts in future versions.

Exit codes:

```text
0 = verification passed
1 = trace exists but verification failed
2 = trace is missing or structurally invalid
```

Example tamper check:

```bash
tracebox run -- echo hello
tracebox verify <trace-id>

echo tampered >> .traces/<trace-id>/stdout.log
tracebox verify <trace-id>
echo $?
```

Expected after tampering:

```text
Status: FAILED
1
```

---

## Custom trace root

By default Tracebox writes traces to `.traces/` in the current directory.

Use another trace directory with:

```bash
tracebox --trace-root /tmp/traces run -- cargo test
tracebox --trace-root /tmp/traces list
```

---

## Retry lineage

Tracebox supports parent trace IDs:

```bash
tracebox run --parent <previous-trace-id> -- cargo test
```

This records lineage metadata in the manifest.

Future tooling can use this to build retry graphs and failure evolution views.

---

## What Tracebox captures

For every run, Tracebox records:

- trace ID;
- optional parent trace ID;
- command argv;
- current working directory;
- start timestamp;
- finish timestamp;
- duration;
- exit code;
- stdout artifact path;
- stderr artifact path;
- artifact SHA-256 hashes;
- manifest SHA-256 sidecar;
- git branch before and after;
- git commit before and after;
- git dirty state before and after;
- workspace dirty file state before and after;
- conservative workspace mutation diff;
- allowlisted environment variables.

Tracebox intentionally does not persist all environment variables.

Environment capture is allowlist-only to avoid leaking secrets such as:

- API keys;
- cloud credentials;
- SSH material;
- access tokens.

---

## Workspace mutation model

Tracebox uses before/after git snapshots.

The model is intentionally conservative.

A file is attributed to the execution only when it:

- appears after the run;
- disappears after the run;
- changes coarse dirty state between snapshots.

Tracebox does not claim that already-dirty files were modified by the traced
command unless their coarse state changes.

Tracebox also ignores its own trace artifact directory so `.traces/` does not
pollute workspace evidence.

---

## Why stdout/stderr are streamed

Tracebox does not use `Command::output()` for command execution.

That API buffers stdout and stderr in memory.

Tracebox instead uses:

```text
spawn()
+ stdout/stderr pipes
+ reader threads
+ stream-to-artifact files
```

This keeps memory behavior stable for large CI logs and leaves room for future:

- live streaming;
- PTY support;
- remote ingestion;
- embedded agent-runtime observers.

---

## Design rules

Tracebox v0.1 deliberately avoids:

- full deterministic replay;
- PTY capture;
- async runtimes;
- ptrace;
- eBPF;
- containers;
- VM snapshots;
- databases;
- GUI/TUI;
- AI summaries;
- telemetry platforms;
- cloud sync.

The product at this stage is the evidence contract.

---

## Future direction

Tracebox is structured so the evidence layer can later be embedded into runtimes
such as `codex-rs`.

The intended integration shape is:

```text
agent/tool runtime emits lifecycle facts
Tracebox records immutable execution evidence
```

Tracebox should not replace rollout/session persistence.

It should record execution evidence and link back to runtime-owned IDs.

Long-term directions:

- trace diffing improvements;
- retry lineage graphs;
- CI failure investigation;
- flaky execution analysis;
- agent tool-call evidence;
- sandbox/runtime provenance;
- workspace reconstruction helpers;
- OCI/container integration;
- governed execution evidence.
