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
tab opens. Write comments, then press **S** to send the whole review immediately
to the same Pi conversation and close the tab. **q** cancels; comments require
confirmation before being discarded. Your existing `/tuicr` extension is untouched.

To review another folder, use `/revisar --repo ../other-project` or
`/revisar --repo="~/Code/other project"`. Both `--repo <path>` and `--repo=<path>`
accept absolute paths, paths relative to Pi's working directory, and `~`/`~/...`.
Quote paths containing spaces. The review covers the target's whole Git working
tree, even when the path names a subdirectory. Without `--repo`, it uses Pi's
current working directory. If Git cannot discover a repository there, it falls
back to `~/.cfg` with `~` as the worktree, including when `.cfg` is bare. This
fallback reviews tracked dotfiles only, never unrelated untracked home files.
An explicit `--repo` never switches to the fallback. Feedback still goes to the
same conversation and includes the reviewed repository's root; Pi's working
directory is unchanged.

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
  old/new side, base commit, and the selected code excerpt in the agent's feedback.
- Suggested replacements: copy a line or range into an editable suggestion block.
- Diff search, hunk navigation, and comment summary.
- Before Send, revisar checks the snapshot again. Changed files require
  confirmation and add a warning that the original line numbers may be stale.

| Key                 | Action                                          |
| ------------------- | ----------------------------------------------- |
| `j` / `k`, arrows   | Move through lines, files, or comments          |
| `h` / `l`           | Scroll the diff horizontally (nowrap only)      |
| `w`                 | Toggle diff wrap / nowrap                       |
| `Ctrl-d` / `Ctrl-u` | Half-page down / up                             |
| `g` / `G`           | First / last row                                |
| `<number>G`         | Jump to a source line in the current diff       |
| `Tab`               | Focus file sidebar / diff                       |
| `{` / `}`           | Previous / next file                            |
| `[` / `]`           | Previous / next hunk in the file                |
| `/`, then `n` / `N` | Search all diffs; next / previous matching line |
| `c`                 | Line comment; on metadata, file comment         |
| `v`, move, `c`      | Range comment within one hunk and one side      |
| `s`                 | Suggest replacement for current line / range    |
| `C` / `a`           | File / general comment                          |
| `r`                 | Comment summary; `Enter` jumps to the anchor    |
| `i` / `d`           | Edit / delete selected comment                  |
| `S`                 | Send all comments and close                     |
| `q`                 | Cancel without sending                          |
| `?`                 | Help                                            |

Type a line number followed by `G` (for example, `42G`) to jump to that source
line in the current file's diff. New-side line numbers take precedence, with an
old-side fallback for deleted lines/files. If neither side shows the line, the
cursor stays put and a message explains why. `Esc` or any other command clears
the pending number; bare `G` still moves to the last row.

In the diff, hunk jumps (`[`/`]`) place the target hunk at the top of the viewport,
like Neovim's `zt`. `<number>G`, `Ctrl-d/u`, PageDown/PageUp, and search jumps
(`/`, `n`/`N`) recenter the cursor like `zz`. `j/k` and the arrow keys keep normal
line-by-line movement. Near the start of a file, centering stops at the first row.

Diff lines wrap by default. Press `w` to toggle wrapping for this session. Wrapped
continuations keep their source line's highlighting and comment anchors; `j/k`
still move by source row. Paging moves by screen rows, so even a single line
taller than the viewport can be read in full. `h/l` and left/right scrolling are
disabled while wrapping; toggling back restores each file's horizontal position.
The diff title shows `wrap` or `nowrap`. Comment editors and summaries always wrap.

Press `s` on a source line, or select a range with `v` and movement then press
`s`, to open a comment prefilled with the selected code in a `suggestion` block.
The editor opens in Vim Normal mode with the cursor on the first code character. Edit the block to show your
replacement; you can add an explanation outside it. Ranges must stay within one
hunk and one side, just like comments. Suggestions on deleted lines refer to the
old base code. The original excerpt and coordinates remain attached, and `i`
reopens the full suggestion for editing. `S` sends it to the agent as review
feedback; revisar never applies the replacement or changes files itself.

The comment editor is a small modal Vim editor. Comments (and `i` edits) open in
Insert mode; suggestions open in Normal mode. The status line shows `-- INSERT --`
or `-- NORMAL --`, and the terminal caret is a block in Normal mode.

- `Ctrl-s` keeps the comment **in memory** from either mode; `Ctrl-c` discards.
- Insert: `Enter`, `Shift-Enter`, or `Ctrl-j` inserts a newline; `Esc` returns to
  Normal. Arrow keys, Home/End, `Ctrl-a/e`, `Ctrl-w`, and `Ctrl-u` work as before.
- Normal: `Enter` keeps the comment; `Esc` discards the edit (or cancels a
  pending operator). Supported commands: `h/j/k/l` and arrows, `w`/`b`, `0`/`$`,
  `f`/`F`/`t`/`T` and a character (current line only), `i`/`a`/`o`/`O`, `x`,
  and `u` (undo each command or Insert session).
- Operators: `d` (delete) and `c` (change: delete, then Insert) take any of
  those motions, plus `dd`/`cc` for the current line. `dj`/`dk` are linewise;
  `dw` stops at the line end and `cw` keeps the blank after a word, as in Vim.
  A `c` change and the text typed after it undo together.
  There are no counts, registers, visual mode, or `:` commands.
- Bracketed paste inserts at the cursor in either mode.
In the summary, `Ctrl-d/u` scrolls the full selected comment and code excerpt.

## Scope and boundaries

The review is the **net working tree against a base commit**, including staged,
unstaged, and untracked files. The base is HEAD, unless the current branch has
diverged from the target branch (`origin/HEAD`, else local `main`, else
`master`): then it is their merge base, so the branch's committed changes are
reviewed even when the working tree is clean. Git's ignore rules apply to
untracked files; Git's ignore rules apply to untracked files. A staged edit
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
For a separate Git directory, standard `GIT_DIR` and `GIT_WORK_TREE` environment
variables are honored. Setting `REVISAR_TRACKED_ONLY=1` excludes untracked files;
the Pi dotfiles fallback sets these variables only for the review process.

## Validate

```sh
./scripts/check
```

This runs Rust formatting, Clippy, all Rust tests, a release build, and the Pi
integration checks together. It requires Python 3, Node.js 22.18+, `npm`, a globally
installed Pi, `tsc`, and `prettier`. No dependencies are downloaded by the Pi
checks: they discover the installed Pi through `npm root -g` (or `PI_PACKAGE_DIR`)
and generate machine-specific type paths only under `target/`.

To run just the Pi checks after building the binary:

```sh
node scripts/check-pi.mjs
```

When changing the installed loader, also follow your Pi-config validation rules:
`cd ~/.pi/agent/extensions && tsc -p tsconfig.json`.

Tests create disposable Git fixtures using libgit2 (a test-only dependency),
exercise the read-only Git CLI backend, render with Ratatui's test backend, and
run the real binary under a Python 3 PTY. PTY checks cover Send, cancellation,
stale snapshots, stdout isolation, and terminal restoration after SIGTERM.
The extension's Node tests mock Ghostty automation but execute its generated
shell wrapper and verify handoff, cancellation, cleanup, and shutdown behavior.
Registration tests use fake homes and checkouts under `target/` and the installed
Pi's real extension loader. They cover different paths, moved checkouts, `/reload`,
missing/invalid registration, binary resolution, and safe setup reruns.
