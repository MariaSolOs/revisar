# revisar

A personal, source-built TUI for reviewing agentic working-tree changes and
sending actionable comments back to the agent. Inspired by
[tuicr](https://github.com/agavra/tuicr), not a fork or a dependency on it.

## Build and use with Pi

Requires Rust, a C toolchain, Git, and a Unix terminal (macOS or Linux).

```sh
cd /Volumes/git/revisar
cargo build --release --locked
```

The extension lives in your Pi config, not this checkout:

```text
~/.pi/agent/extensions/revisar/index.ts
```

It points directly at this checkout's `target/release/revisar`. After rebuilding,
the next review uses the new binary. If you move the checkout, update `REVISAR`
in the extension.

In Pi, run `/reload`, then `/revisar` after the agent has finished. A new Ghostty
tab opens. Write comments, then press **S** and confirm to send the whole review
to the same Pi conversation and close the tab. **q** cancels; comments require
confirmation before being discarded. Your existing `/tuicr` extension is untouched.

On macOS, the launcher uses Ghostty 1.3's native AppleScript API and needs
Automation permission to control Ghostty. The wrapper runs as the new tab's
command with `wait after command` disabled. Cleanup closes that tab by its
stable ID if it is still open, never whichever tab happens to be focused. The
clipboard is untouched. Linux uses Hyprland's `hyprctl`, `wl-copy`, and your
`alt+shift+t` / `alt+shift+p` bindings to launch an `exec` wrapper; that path
replaces the clipboard. The wrapper records revisar's real exit status for Pi,
then exits successfully so cancellation is not treated as a terminal failure.

## Review flow

- Single-file unified diffs, syntax highlighting, and a file sidebar.
- The exact miss-dracula palette is fixed in `src/theme.rs`. Your syntax theme
  is embedded from `assets/miss-dracula-syntax.tmTheme`; no runtime config files.
- Line, range, file, and general comments. Comments include source coordinates,
  old/new side, base HEAD, and the selected code excerpt in the agent's feedback.
- Diff search, hunk navigation, comment summary, and in-memory reviewed markers.
- Before Send, revisar checks the snapshot again. Changed files require a second
  confirmation and add a warning that the original line numbers may be stale.

| Key                 | Action                                          |
| ------------------- | ----------------------------------------------- |
| `j` / `k`, arrows   | Move through lines, files, or comments          |
| `h` / `l`           | Scroll the diff horizontally                    |
| `Ctrl-d` / `Ctrl-u` | Half-page down / up                             |
| `g` / `G`           | First / last row                                |
| `Tab`               | Focus file sidebar / diff                       |
| `{` / `}`           | Previous / next file                            |
| `[` / `]`           | Previous / next hunk in the file                |
| `/`, then `n` / `N` | Search all diffs; next / previous matching line |
| `c`                 | Line comment; on metadata, file comment         |
| `v`, move, `c`      | Range comment within one hunk and one side      |
| `C` / `a`           | File / general comment                          |
| `s`                 | Comment summary; `Enter` jumps to the anchor    |
| `i` / `d`           | Edit / delete selected comment                  |
| `r`                 | Toggle file reviewed                            |
| `S`                 | Send all comments and close                     |
| `q`                 | Cancel without sending                          |
| `?`                 | Help                                            |

In the comment editor, `Enter` or `Ctrl-s` keeps the comment **in memory**;
`Shift-Enter` or `Ctrl-j` inserts a newline; `Esc` discards that edit. Arrow keys,
Home/End, `Ctrl-a/e`, `Ctrl-w`, `Ctrl-u`, and bracketed paste are supported.
In the summary, `Ctrl-d/u` scrolls the full selected comment and code excerpt.

## Scope and boundaries

The review is the **net working tree against HEAD**, including staged, unstaged,
and untracked files; Git's ignore rules apply to untracked files. A staged edit
that is undone in the working tree has no net change and is not shown. revisar
cannot attribute edits to a particular agent: all local changes are included.
Unborn repositories and linked worktrees work too.

Git is read-only: no staging, commits, resets, fetches, hooks, or external diff /
textconv programs. Renames appear as deletion plus addition. Binary and mode-only
changes can receive file comments; binary contents are not rendered. Diffs show
five context lines per hunk, without expandable context. Syntax parser state
restarts at each hunk, so highlighting inside a multi-line construct that began
in omitted code can be approximate. Merge conflicts, non-UTF-8 filenames, and
reviews above 32 MiB of diff output are rejected explicitly. Nested untracked
repositories are not traversed; tracked submodules show Git's short summary.

There are no saved/resumable reviews, hosting integrations, comment categories,
side-by-side mode, mouse controls, external-editor launching, theme settings,
telemetry, installers, release binaries, or updater.

Comments and reviewed markers exist only in the running TUI. The extension uses
a private temporary directory for a wrapper, one-shot feedback, and an atomic
exit marker; it removes that transport on completion, cancellation, error, or
Pi shutdown/reload. It never uses tuicr's session store or changes HOME/XDG paths.
Pi retains submitted feedback as ordinary conversation text, not as a revisar
session. If Pi switches/reloads, the review is abandoned and its macOS tab is
closed; on Linux, close any remaining review tab manually. Reviews time out
after four hours.
As with any process, a hard kill or machine crash can leave temporary files;
there is no recovery/resume mechanism.

## Standalone / agent transport

The TUI always renders to `/dev/tty`; stdout contains only submitted Markdown.
Run the built binary inside the repository to review. To capture feedback:

```sh
/path/to/revisar/target/release/revisar > /tmp/feedback.md
```

Place redirected output outside the reviewed repository so it doesn't appear
as an untracked change. Exit codes: `0` sent, `1` error, `2` cancelled. Cancellation
emits no feedback. With no comments, `S` does nothing; `q` closes without implying
approval or starting an agent turn. `--help` and `--version` are the only CLI flags.

## Validate

```sh
cargo fmt --all -- --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --all-targets --locked

cd ~/.pi/agent/extensions
tsgo -p tsconfig.json
prettier --check revisar/index.ts revisar/index.test.mjs
node --experimental-test-module-mocks --test revisar/index.test.mjs
```

Tests create disposable Git fixtures using libgit2 (a test-only dependency),
exercise the read-only Git CLI backend, render with Ratatui's test backend, and
run the real binary under a Python 3 PTY. PTY checks cover Send, cancellation,
stale snapshots, stdout isolation, and terminal restoration after SIGTERM.
The extension's Node tests mock Ghostty automation but execute its generated
shell wrapper and verify handoff, cancellation, cleanup, and shutdown behavior.
