use std::io::{BufReader, Read};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::thread;

use anyhow::{anyhow, Context, Result};

use crate::evidence::{EvidenceRecorder, OutputStream, ToolFinished, ToolStarted, TraceManifest};

/// Execute a command and stream stdout/stderr into the evidence recorder.
///
/// This function intentionally does not use `Command::output()`.
///
/// `Command::output()` buffers the entire stdout/stderr in memory. That is a
/// bad default for CI logs, compiler output, and agent tool executions. Here we
/// spawn the process, read output from pipes in dedicated threads, and write
/// chunks directly into trace artifacts.
pub fn execute_command(
    recorder: Arc<dyn EvidenceRecorder>,
    started: ToolStarted,
) -> Result<TraceManifest> {
    let handle = recorder.tool_started(started.clone())?;

    let program = started
        .command
        .first()
        .ok_or_else(|| anyhow!("missing command program"))?
        .clone();

    let args = started.command[1..].to_vec();

    let spawn_result = Command::new(&program)
        .args(&args)
        .current_dir(&started.cwd)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn();

    let mut child = match spawn_result {
        Ok(child) => child,
        Err(error) => {
            // Spawn failures are also useful evidence. Record them into stderr
            // and finalize the trace with no exit code.
            let message = format!("tracebox: failed to spawn command `{program}`: {error}\n");
            let _ = recorder.tool_output(&handle, OutputStream::Stderr, message.as_bytes());

            return recorder.tool_finished(handle, ToolFinished { exit_code: None });
        }
    };

    let stdout = child
        .stdout
        .take()
        .context("child stdout pipe was not available")?;

    let stderr = child
        .stderr
        .take()
        .context("child stderr pipe was not available")?;

    let stdout_recorder = Arc::clone(&recorder);
    let stdout_handle = handle.clone();

    let stdout_thread = thread::spawn(move || {
        stream_reader(stdout, stdout_recorder, stdout_handle, OutputStream::Stdout)
    });

    let stderr_recorder = Arc::clone(&recorder);
    let stderr_handle = handle.clone();

    let stderr_thread = thread::spawn(move || {
        stream_reader(stderr, stderr_recorder, stderr_handle, OutputStream::Stderr)
    });

    let status = child.wait().context("failed to wait for child process")?;

    stdout_thread
        .join()
        .map_err(|_| anyhow!("stdout reader thread panicked"))??;

    stderr_thread
        .join()
        .map_err(|_| anyhow!("stderr reader thread panicked"))??;

    recorder.tool_finished(
        handle,
        ToolFinished {
            exit_code: status.code(),
        },
    )
}

fn stream_reader<R: Read + Send + 'static>(
    reader: R,
    recorder: Arc<dyn EvidenceRecorder>,
    handle: crate::evidence::recorder::ToolTraceHandle,
    stream: OutputStream,
) -> Result<()> {
    let mut reader = BufReader::new(reader);
    let mut buffer = [0_u8; 16 * 1024];

    loop {
        let read = reader
            .read(&mut buffer)
            .with_context(|| format!("failed to read {:?} stream", stream))?;

        if read == 0 {
            break;
        }

        recorder.tool_output(&handle, stream, &buffer[..read])?;
    }

    Ok(())
}
