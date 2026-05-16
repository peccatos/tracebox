# Tracebox CI Artifacts

Tracebox traces are designed to be uploaded as CI artifacts when a job finishes.

Minimal GitHub Actions pattern:

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

This keeps the preserved evidence available even when the test command fails.
