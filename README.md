# Tracebox

Tracebox is a Linux-native immutable execution evidence runtime.

It records deterministic evidence about process executions so failed runs, retries,
workspace mutations, and flaky behavior can be inspected later.

Tracebox does **not** attempt full deterministic execution replay.

The core idea is:

```text
execution replay is often impossible
evidence replay is possible
```

Tracebox focuses on:

- command execution evidence;
- immutable trace bundles;
- stdout/stderr artifacts;
- git and workspace state;
- trace inspection;
- trace diffing;
- artifact integrity verification;
- machine-readable JSON output for automation.

It is not:

- a terminal video recorder;
- a screen recorder;
- a chat transcript viewer;
- a full deterministic replay system;
- an AI observability dashboard.

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

## Basic usage

Run a command and capture evidence:

```bash
tracebox run -- cargo test
```

This creates an immutable trace bundle under `.traces/`:

```text
.traces/
└── trc_<uuidv7>/
    ├── manifest.json
    ├── manifest.sha256
    ├── stdout.log
    └── stderr.log
```

Tracebox preserves the wrapped command exit code.

For example:

```bash
tracebox run -- sh -c 'echo ok && echo err >&2 && exit 7'
echo $?
```

Expected:

```text
7
```

The trace is still written even when the wrapped command fails.

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
