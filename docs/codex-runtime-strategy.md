# Tracebox × codex-rs Runtime Strategy

## Status

Tracebox is currently a standalone alpha execution evidence runtime.

It already supports:

- command execution capture;
- immutable trace bundles;
- stdout/stderr artifacts;
- git/workspace evidence;
- trace inspection;
- trace diffing;
- artifact integrity verification;
- manifest semantic validation;
- JSON output;
- CI;
- packaged Linux release artifacts.

Current public alpha baseline:

```text
v0.1.0-alpha.6
```

Tracebox should not be presented as a finished Codex integration yet.

The next major milestone is a local embedded runtime proof inside or adjacent to
`codex-rs`.

---

## Core Position

Tracebox is not a replay engine.

Tracebox is not a transcript viewer.

Tracebox is not a replacement for rollout/session persistence.

Tracebox is an execution evidence runtime.

The central idea is:

```text
execution replay is often impossible
evidence replay is possible
```

For Codex-like systems, Tracebox should capture runtime evidence around
side-effecting tool execution.

---

## Why Not Show Too Early

A standalone CLI can be misunderstood as:

- a command logger;
- a wrapper around shell commands;
- a debugging toy;
- an alternative transcript format;
- an attempted deterministic replay system.

That is not the intended value.

Tracebox should be shown to the Codex team only after it demonstrates value on
real or local `codex-rs` execution flows.

The desired proof is:

```text
codex-rs tool execution
↓
Tracebox evidence observer
↓
immutable trace bundle
↓
inspect / diff / verify / validate
```

---

## What Must Be Proven Before Showing

Tracebox should not be pitched until the following gates pass locally.

### Gate 1: codex-rs execution creates a trace

A local `codex-rs` tool or shell execution should produce:

```text
.traces/
└── trc_<uuidv7>/
    ├── manifest.json
    ├── manifest.sha256
    ├── stdout.log
    └── stderr.log
```

The manifest must include at minimum:

- trace ID;
- command argv;
- cwd;
- started_at;
- finished_at;
- duration_ms;
- exit_code;
- artifact paths;
- git state;
- workspace state.

---

### Gate 2: stdout/stderr are captured correctly

The produced trace must preserve actual tool output:

```text
stdout.log = actual stdout bytes
stderr.log = actual stderr bytes
```

Tracebox must not rely on transcript rendering.

Tracebox must capture runtime output at or near the execution boundary.

---

### Gate 3: workspace mutation evidence is useful

For a Codex task that modifies files, Tracebox must show:

- dirty_before;
- dirty_after;
- created files;
- modified files;
- deleted files.

Example:

```bash
tracebox inspect <trace-id>
```

Should reveal workspace changes caused by the tool execution.

---

### Gate 4: trace integrity passes

Every trace generated from local `codex-rs` execution must pass:

```bash
tracebox verify <trace-id>
```

This proves artifact bytes match recorded hashes.

---

### Gate 5: manifest validation passes

Every trace generated from local `codex-rs` execution must pass:

```bash
tracebox validate <trace-id>
```

This proves the trace bundle is structurally and semantically valid.

---

### Gate 6: trace can be linked to Codex runtime context

The manifest should contain, when available:

```json
{
  "context": {
    "thread_id": "...",
    "turn_id": "...",
    "item_id": "...",
    "tool_call_id": "...",
    "parent_trace_id": "..."
  }
}
```

Not every field has to be available in the first local patch.

But at least one stable runtime execution identifier must be captured.

For early local proof, acceptable identifiers include:

- process ID;
- tool call ID;
- command execution ID;
- rollout item ID;
- internal event ID.

---

### Gate 7: Tracebox gives value beyond transcript

A demo must show something that rollout/transcript alone does not provide.

Examples:

- exact stdout/stderr artifacts;
- exit code;
- artifact hashes;
- dirty_before vs dirty_after;
- created/modified/deleted files;
- trace integrity verification;
- manifest validation;
- diff between failed attempt and retry.

The point is not to duplicate transcript.

The point is to capture runtime evidence.

---

## Integration Philosophy

Tracebox should integrate with `codex-rs` as an observer layer.

Correct dependency direction:

```text
codex-rs runtime
    emits lifecycle facts
        ↓
Tracebox evidence recorder
    writes immutable evidence
```

Incorrect dependency direction:

```text
Tracebox owns Codex session logic
Tracebox replaces rollout
Tracebox wraps all Codex execution externally
```

Tracebox must not own Codex conversation/session state.

Codex owns:

- model loop;
- rollout;
- transcript;
- session history;
- approvals;
- UI;
- policy;
- tool orchestration.

Tracebox owns:

- execution evidence;
- stdout/stderr artifacts;
- process/tool runtime facts;
- workspace mutation evidence;
- integrity metadata;
- validation metadata;
- trace storage layout.

---

## Initial Local Integration Target

The first local integration should target the tool/process execution boundary.

Avoid starting from:

- TUI;
- app-server UI;
- model loop;
- high-level `codex exec` wrapper;
- transcript persistence.

Prefer the layer where these facts exist:

```text
process/tool started
stdout chunk
stderr chunk
process/tool exited
process/tool closed
```

The ideal boundary exposes:

- command argv;
- cwd;
- stdout bytes;
- stderr bytes;
- exit code;
- process/tool ID;
- lifecycle timing.

---

## Standalone Wrapper Is Not Enough

This is useful for smoke testing:

```bash
tracebox run -- codex exec "..."
```

But it is not sufficient proof.

It only captures Codex as one outer process.

It does not capture individual tool executions.

The stronger proof is embedded or adjacent instrumentation:

```text
Codex tool execution
    -> ToolStarted
    -> ToolOutput(stdout/stderr)
    -> ToolFinished
    -> Tracebox bundle
```

---

## Proposed Evidence API Shape

The embedded boundary should eventually look like:

```rust
pub trait EvidenceRecorder {
    fn tool_started(&self, event: ToolStarted) -> Result<ToolTraceHandle>;

    fn tool_output(
        &self,
        handle: &ToolTraceHandle,
        stream: OutputStream,
        bytes: &[u8],
    ) -> Result<()>;

    fn tool_finished(
        &self,
        handle: ToolTraceHandle,
        event: ToolFinished,
    ) -> Result<TraceManifest>;
}
```

Core events:

```rust
pub struct ToolStarted {
    pub tool_kind: String,
    pub command: Vec<String>,
    pub cwd: PathBuf,
    pub context: ExecutionContext,
}

pub struct ToolFinished {
    pub exit_code: Option<i32>,
}

pub enum OutputStream {
    Stdout,
    Stderr,
    Pty,
}
```

Tracebox must treat recorder failure as best-effort by default.

A failure to write evidence should not break Codex tool execution during early
integration.

---

## Local Dogfood Plan

### Phase 1: Prepare codex-rs checkout

```bash
git clone https://github.com/openai/codex.git
cd codex/codex-rs
```

Goal:

```bash
cargo build
cargo test
```

If the full workspace is too heavy, identify the smallest relevant crate subset.

---

### Phase 2: Locate execution boundary

Search areas:

```bash
rg "process/start"
rg "process/output"
rg "process/exited"
rg "stdout"
rg "stderr"
rg "exit_code"
rg "spawn"
rg "ToolCall"
rg "shell"
```

Likely areas:

```text
exec-server/
shell-command/
tools/
core/
sandboxing/
linux-sandbox/
app-server/
```

Goal:

Find where command execution lifecycle is emitted or stored.

---

### Phase 3: Local patch

Do not open upstream PR yet.

Patch locally to write Tracebox traces from the execution boundary.

Acceptable local-only strategies:

1. depend on local Tracebox crate path;
2. vendor `tracebox` evidence module temporarily;
3. call a local evidence adapter crate.

The preferred direction is a reusable library-style evidence recorder, not
spawning the `tracebox` binary for every command.

---

### Phase 4: Run real Codex tasks

Use tasks that produce meaningful evidence:

- successful shell command;
- failing shell command;
- command with stderr;
- command that modifies a file;
- retry after failure;
- command with no output;
- command that creates/deletes files.

For each produced trace:

```bash
tracebox inspect <trace-id> --stdout --stderr
tracebox verify <trace-id>
tracebox validate <trace-id>
```

For retry analysis:

```bash
tracebox diff <trace-a> <trace-b>
tracebox diff <trace-a> <trace-b> --json
```

---

### Phase 5: Write integration findings

Produce a short local report:

```text
docs/codex-local-runtime-findings.md
```

It should include:

- where the execution boundary was found;
- which IDs are available;
- what evidence was captured;
- what Tracebox adds beyond transcript;
- limitations;
- proposed upstream integration path.

---

## Demo Criteria

A convincing demo should show:

```text
1. Codex runs a tool command.
2. Tracebox creates a trace bundle.
3. The trace captures stdout/stderr/exit code.
4. The trace captures workspace mutation.
5. `verify` passes.
6. `validate` passes.
7. `diff` explains a retry or failure change.
8. JSON output can be consumed by tooling.
```

The demo should be runnable locally.

No cloud service should be required.

No UI should be required.

---

## What To Say When Showing Later

Use this framing:

```text
Tracebox is an execution evidence layer for agent/tool runtimes.

It does not attempt deterministic execution replay.

It records immutable runtime evidence around tool executions:
stdout/stderr, exit code, git/workspace state, artifact hashes, and validation metadata.

We tested it locally against codex-rs execution flows and can show trace bundles
that provide evidence beyond rollout/transcript history.
```

Do not say:

```text
Tracebox is a Codex replacement.
Tracebox replaces rollout.
Tracebox replays executions.
Tracebox is an AI dashboard.
Tracebox is production-ready for upstream.
```

---

## Current Decision

Do not present Tracebox to the Codex team yet as a proposal.

Continue until local `codex-rs` runtime proof exists.

The next milestone is:

```text
Tracebox embedded evidence adapter for local codex-rs
```

This milestone is complete only when Tracebox evidence is produced from actual
local Codex tool/process execution, not from wrapping the outer Codex command.
