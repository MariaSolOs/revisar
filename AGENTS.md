# revisar

Personal, source-only Rust TUI. Optimize for reviewing agentic working-tree
changes and sending comments immediately to the agent, not general code review.

## Scope

- Git working tree only: net staged + unstaged + untracked against HEAD.
- No hosting integrations, persisted/resumable reviews, distribution machinery,
  updater, configurable themes, or Git mutations.
- Fixed miss-dracula UI palette and embedded syntax theme.
- Ask before adding features outside the existing review flow.
- The Pi implementation and tests belong in `integrations/pi/`. Pi config contains
  only the managed loader from `integrations/pi/loader.ts`; never maintain a second
  implementation there. Read applicable Pi config instructions before changing it.
- `node scripts/setup-pi.mjs` registers this checkout in machine-local state and
  installs the path-independent loader. Never put machine-specific paths in source
  or synced config. The registration stores a path, not review/session data.

## Code

- `src/diff.rs`: read-only Git CLI snapshot and unified diff coordinates.
- `src/review.rs`: comment anchors, excerpts, and agent feedback.
- `src/app.rs`: in-memory state and key handling; `src/editor.rs`: comment input.
- `src/ui.rs`, `src/theme.rs`: rendering and embedded theme.
- `src/main.rs`: terminal lifecycle; `/dev/tty` for UI, stdout for feedback only.
- `tests/`: disposable Git fixtures and real PTY tests. Never mutate a user's repo.
- `integrations/pi/`: Pi integration, managed loader template, and Node tests.
- `scripts/setup-pi.mjs`: machine-local checkout registration and loader setup.
- `scripts/check`: validates the binary and Pi integration together; Pi checks use
  the installed Pi's types and loader, not a second npm dependency copy.

## Validation

```sh
./scripts/check
```

This includes `cargo fmt`, Clippy with warnings denied, locked tests and release
build, `tsc`, Prettier, and Node integration tests. For only the Pi checks after
building, use `node scripts/check-pi.mjs`. Node.js 22.18+, globally installed Pi,
`npm`, `tsc`, and `prettier` are required.

Python 3 is needed for PTY tests. Keep Cargo.lock checked in. Test fixtures stay
under `target/`. There is no Git repository initialized in a fresh checkout
until the owner initializes it; do not initialize one on their behalf.
