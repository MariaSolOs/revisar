# revisar

A personal, source-built TUI for reviewing agentic working-tree changes and
sending actionable comments back to the agent. Inspired by
[tuicr](https://github.com/agavra/tuicr), not a fork or a dependency on it.

## Build and use with Pi

Requires Rust, a C toolchain, Git, and a Unix terminal (macOS or Linux).

From your revisar checkout, wherever it lives on this computer:

```sh
cargo build --release --locked
node scripts/setup-pi.mjs
```

The setup command installs a tiny, path-independent loader at
`~/.pi/agent/extensions/revisar/index.ts`. Keep that loader in your normal synced
Pi config. The implementation and tests stay here in `integrations/pi/`; they
are never copied into your config.

Each computer stores its own checkout location in
`$XDG_STATE_HOME/revisar/checkout.json`, defaulting to
`~/.local/state/revisar/checkout.json`. **Do not sync this registration file.**
It contains only the checkout path, not review data. Setup also honors
`PI_CODING_AGENT_DIR` for a non-default Pi config directory. Use the same
`XDG_STATE_HOME` when running setup and Pi; relative XDG values are ignored.

The loader imports directly from the registered checkout. The extension finds
`target/release/revisar` relative to its own source, so a different checkout path
on each computer needs no config edits. No npm package installation is involved.
Node.js 22.18+ is needed for setup and integration tests.

On a new computer or after moving the checkout, rerun the two commands above,
then `/reload` in Pi. For ordinary code updates, rebuild the binary and `/reload`;
setup is only needed again after a move or a change to the loader template.
Only one checkout can be registered per local state directory; the last setup wins.

Setup is safe to repeat and only replaces its own managed loader. When migrating
from the old full extension in Pi config, first preserve any local changes and
move that old extension directory outside Pi's auto-discovery directory. Setup
refuses to overwrite an unmanaged extension rather than silently losing edits.
Do not also add the implementation to Pi's `settings.json`: that would register
`/revisar` twice. If no valid checkout is registered, `/revisar` explains how to
set it up instead of breaking Pi startup.

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
- Diff search, hunk navigation, and comment summary.
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
| `S`                 | Send all comments and close                     |
| `q`                 | Cancel without sending                          |
| `?`                 | Help                                            |

In the diff, `Ctrl-d/u`, PageDown/PageUp, hunk jumps (`[`/`]`), and search jumps
(`/`, `n`/`N`) recenter the cursor like Neovim's `zz`. `j/k` and the arrow keys keep normal
line-by-line movement. Near the start of a file, centering stops at the first row.

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
telemetry, release installers, release binaries, or updater.

Comments exist only in the running TUI. The extension uses
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
./scripts/check
```

This runs Rust formatting, Clippy, all Rust tests, a release build, and the Pi
integration checks together. It requires Python 3, Node.js 22.18+, `npm`, a globally
installed Pi, `tsgo`, and `prettier`. No dependencies are downloaded by the Pi
checks: they discover the installed Pi through `npm root -g` (or `PI_PACKAGE_DIR`)
and generate machine-specific type paths only under `target/`.

To run just the Pi checks after building the binary:

```sh
node scripts/check-pi.mjs
```

When changing the installed loader, also follow your Pi-config validation rules:
`cd ~/.pi/agent/extensions && tsgo -p tsconfig.json`.

Tests create disposable Git fixtures using libgit2 (a test-only dependency),
exercise the read-only Git CLI backend, render with Ratatui's test backend, and
run the real binary under a Python 3 PTY. PTY checks cover Send, cancellation,
stale snapshots, stdout isolation, and terminal restoration after SIGTERM.
The extension's Node tests mock Ghostty automation but execute its generated
shell wrapper and verify handoff, cancellation, cleanup, and shutdown behavior.
Registration tests use fake homes and checkouts under `target/` and the installed
Pi's real extension loader. They cover different paths, moved checkouts, `/reload`,
missing/invalid registration, binary resolution, and safe setup reruns.
