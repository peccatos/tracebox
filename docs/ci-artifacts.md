# Tracebox CI Artifacts

Tracebox traces are designed to be uploaded as CI artifacts when a job finishes.
Tracebox dogfoods this pattern in its own GitHub Actions workflow by building the
binary first, running tests through `tracebox run`, and uploading `.traces/`
even when the wrapped test command fails.

Minimal GitHub Actions pattern:

```yaml
- name: Build Tracebox
  run: cargo build --locked --all-features

- name: Test with Tracebox
  run: ./target/debug/tracebox run -- cargo test --locked --all-features

- name: Upload Tracebox evidence
  if: always()
  uses: actions/upload-artifact@v4
  with:
    name: tracebox-ci-evidence
    path: .traces/
    include-hidden-files: true
    if-no-files-found: ignore

Because `.traces/` is a hidden directory, `actions/upload-artifact` needs `include-hidden-files: true`.

- name: Upload Tracebox evidence
  if: always()
  uses: actions/upload-artifact@v4
  with:
    name: tracebox-ci-evidence
    path: .traces/
    if-no-files-found: ignore
```

This keeps the preserved evidence available even when the test command fails.
