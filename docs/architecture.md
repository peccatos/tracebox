# Tracebox architecture

## Core principle

Tracebox does not try to replay execution.

It records immutable execution evidence.

```text
Execution replay is often impossible.
Evidence replay is possible.
```

## v0.1 boundaries

The standalone repository owns:

- CLI;
- evidence manifest;
- filesystem trace storage;
- stdout/stderr artifact capture;
- conservative workspace diffing;
- inspect/list/diff commands.

It deliberately avoids:

- PTY;
- async runtime;
- kernel tracing;
- sandboxing;
- databases;
- UI;
- cloud sync.

## Important invariants

### Trace bundles are append-only

A trace directory is created once. Artifacts are created with `create_new`.
`manifest.json` is written once. `manifest.sha256` is written as a sidecar.

### Process output is streamed

The runner uses `spawn()` with piped stdout/stderr and reader threads.
It does not use `Command::output()`.

### Workspace attribution is conservative

Tracebox captures workspace state before and after execution.
If a file was already dirty before execution and remains dirty with the same coarse state,
v0.1 does not claim the command changed it.

A future version can hash dirty files before and after to improve attribution.

### Environment capture is allowlist-only

Tracebox must not blindly persist all environment variables.
