# Diagnosing environment drift with Tracebox

Tracebox is useful when the same command behaves differently across local and CI environments. This demo shows a tiny Rust crate whose test depends on `TRACEBOX_MODE`.

## Problem

Environment drift is easy to miss when a command succeeds locally and fails in CI. Tracebox preserves the evidence from both runs so you can compare what changed instead of guessing.

## Setup

```bash
git clone https://github.com/peccatos/tracebox.git
cd tracebox
cargo build

cd examples/env-drift
```

## Passing local-like run

```bash
TRACEBOX_MODE=stable ../../target/debug/tracebox run -- cargo test
```

This should pass and create a trace bundle for the successful run.

## Failing CI-like run

```bash
TRACEBOX_MODE=broken ../../target/debug/tracebox run -- cargo test
```

This should fail with a non-zero exit code, and Tracebox still records the run.

## Inspecting traces

```bash
../../target/debug/tracebox list
```

Use the trace IDs from the list to inspect the preserved evidence.

## Comparing traces

```bash
../../target/debug/tracebox diff <passed-trace-id> <failed-trace-id>
```

The diff should highlight the exit code change, stderr differences, and the environment delta for `TRACEBOX_MODE`.

## Generating report

```bash
../../target/debug/tracebox report <failed-trace-id>
cat .traces/<failed-trace-id>/report.md
```

The report keeps the failure evidence together in Markdown form.

## Opening TUI

From the repository root:

```bash
cargo run --features tui -- tui
```

If you are using an installed binary built with `--features tui`, run:

```bash
../../target/debug/tracebox tui
```

The TUI lets you browse active and archived traces, inspect summaries, verify integrity, generate reports, and archive or restore evidence.

## Expected diagnosis

The failed run should show:

- exit code changed;
- stderr changed;
- environment variable `TRACEBOX_MODE` differs;
- the failed run still has preserved stdout, stderr, manifest, and report evidence.

## Why this matters for CI/Codex/agentic workflows

This pattern is the common failure mode in automation: the command itself is not the whole story. Tracebox makes drift visible so a human or agent can compare runs, spot environment-dependent behavior, and preserve a reproducible evidence trail.
