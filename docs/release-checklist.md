# Release Checklist

Before cutting the next alpha release:

- `git status -sb`
- `cargo fmt --check`
- `cargo clippy --all-targets -- -D warnings`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo test --locked`
- `cargo test --locked --all-features`
- `cargo build --release --locked`
- env drift demo smoke
- TUI smoke
- tag release
- push tag
- verify GitHub release artifacts

