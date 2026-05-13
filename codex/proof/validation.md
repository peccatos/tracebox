# Codex app-server Tracebox evidence proof

This PR adds a portable proof artifact for embedding Tracebox-style command execution evidence into `codex-rs/app-server`.

Upstream Codex currently accepts external code contributions by invitation only, so this repository keeps the integration as a patch/proof artifact instead of opening an unsolicited upstream Codex PR.

## Patch

`codex/patches/0001-Record-Tracebox-evidence-for-app-server-command-exec.patch`

## What the patch does

- Adds an opt-in Tracebox evidence recorder to `codex-rs/app-server`.
- Records command/exec stdout and stderr into artifact files.
- Writes `manifest.json` with command metadata, cwd, source path, exit code, and SHA-256 hashes.
- Writes `manifest.sha256` for manifest integrity.
- Uses `TRACEBOX_ENABLED=1` and `TRACEBOX_ROOT=/path/to/root`.
- Does not take ownership of Codex process execution, sandboxing, cancellation, streaming, or JSON-RPC response semantics.

## Local validation

Validated from local Codex checkout:

- branch: `local/tracebox-evidence-proof`
- commit: `6bee428 Record Tracebox evidence for app-server command exec`

Commands run:

    cargo check -p codex-app-server --locked

    cargo test -p codex-app-server tracebox_evidence --locked

    rm -rf /tmp/codex-tracebox-proof

    TRACEBOX_ENABLED=1 \
    TRACEBOX_ROOT=/tmp/codex-tracebox-proof \
    cargo test -p codex-app-server \
      suite::v2::command_exec::command_exec_without_process_id_keeps_buffered_compatibility \
      --locked -- --nocapture

Observed generated files:

    /tmp/codex-tracebox-proof/trc_<uuidv7>/manifest.json
    /tmp/codex-tracebox-proof/trc_<uuidv7>/manifest.sha256
    /tmp/codex-tracebox-proof/trc_<uuidv7>/stderr.log
    /tmp/codex-tracebox-proof/trc_<uuidv7>/stdout.log

Observed artifact contents:

    stdout.log: legacy-out
    stderr.log: legacy-err

Artifact integrity was verified by comparing `stdout.log` and `stderr.log` SHA-256 digests against hashes recorded in `manifest.json`.
