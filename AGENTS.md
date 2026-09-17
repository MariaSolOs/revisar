# revisar

Personal, source-only Rust TUI. Optimize for reviewing agentic working-tree
changes and sending comments immediately to the agent, not general code review.

## Scope

- Git working tree only: net staged + unstaged + untracked against HEAD.
- No hosting integrations, persisted/resumable reviews, distribution machinery,
  updater, configurable themes, or Git mutations.
- Fixed miss-dracula UI palette and embedded syntax theme.
- Ask before adding features outside the existing review flow.
- The Pi extension belongs in `~/.pi/agent/extensions/revisar/index.ts`, not here.
  Read the applicable Pi config instructions before changing it.

## Code

- `src/diff.rs`: read-only Git CLI snapshot and unified diff coordinates.
- `src/review.rs`: comment anchors, excerpts, and agent feedback.
- `src/app.rs`: in-memory state and key handling; `src/editor.rs`: comment input.
- `src/ui.rs`, `src/theme.rs`: rendering and embedded theme.
- `src/main.rs`: terminal lifecycle; `/dev/tty` for UI, stdout for feedback only.
- `tests/`: disposable Git fixtures and real PTY tests. Never mutate a user's repo.

## Validation

```sh
cargo fmt --all -- --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --all-targets --locked
cargo build --release --locked
```

Python 3 is needed for PTY tests. Keep Cargo.lock checked in. Test fixtures stay
under `target/`. There is no Git repository initialized in a fresh checkout
until the owner initializes it; do not initialize one on their behalf.
