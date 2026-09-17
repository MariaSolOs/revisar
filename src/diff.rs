use anyhow::{Context, Result, bail, ensure};
use std::{
    collections::BTreeSet,
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

// Bound memory without silently presenting a partial review as complete.
const MAX_OUTPUT: u64 = 32 * 1024 * 1024;
const DIFF_OPTIONS: &[&str] = &[
    "--no-ext-diff",
    "--no-textconv",
    "--no-color",
    "--no-renames",
    "--no-relative",
    "--full-index",
    "--unified=5",
    "--ignore-submodules=none",
    "--src-prefix=a/",
    "--dst-prefix=b/",
    "--submodule=short",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Context,
    Add,
    Delete,
    Hunk,
    Meta,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    pub kind: Kind,
    pub text: String,
    pub old: Option<usize>,
    pub new: Option<usize>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileDiff {
    pub path: String,
    pub status: char,
    pub rows: Vec<Row>,
    // Preserve the exact bytes (including blob hashes) for the send-time check.
    pub patch: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Snapshot {
    pub root: PathBuf,
    pub head: String,
    pub files: Vec<FileDiff>,
}

fn git(root: &Path, args: &[&str], allowed: &[i32]) -> Result<Vec<u8>> {
    let mut child = Command::new("git")
        .current_dir(root)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_LITERAL_PATHSPECS", "1")
        .env("LC_ALL", "C")
        .args([
            "--no-pager",
            "-c",
            "diff.external=",
            "-c",
            "diff.suppressBlankEmpty=false",
        ])
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("Could not run git")?;
    // Drain stderr concurrently so neither pipe can deadlock the other.
    let mut stderr = child.stderr.take().unwrap();
    let errors = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = stderr.by_ref().take(MAX_OUTPUT).read_to_end(&mut bytes);
        bytes
    });
    let mut bytes = Vec::new();
    let read = child
        .stdout
        .take()
        .unwrap()
        .take(MAX_OUTPUT + 1)
        .read_to_end(&mut bytes);
    if read.is_err() || bytes.len() as u64 > MAX_OUTPUT {
        let _ = child.kill();
        let _ = child.wait();
        let _ = errors.join();
        read?;
        bail!("Git output exceeds 32 MiB; reduce the working-tree changes before reviewing");
    }
    let status = child.wait()?;
    let errors = errors.join().unwrap_or_default();
    ensure!(
        status.code().is_some_and(|c| allowed.contains(&c)),
        "git {}: {}",
        args.join(" "),
        String::from_utf8_lossy(&errors).trim()
    );
    Ok(bytes)
}

fn paths(bytes: &[u8]) -> Result<BTreeSet<String>> {
    bytes
        .split(|b| *b == 0)
        .filter(|p| !p.is_empty())
        .map(|p| String::from_utf8(p.to_vec()).context("Non-UTF-8 filenames are not supported"))
        .collect()
}

impl Snapshot {
    pub fn load(cwd: &Path) -> Result<Self> {
        let root = git(cwd, &["rev-parse", "--show-toplevel"], &[0])?;
        // Strip only Git's terminating LF, not whitespace belonging to the path.
        let root = root.strip_suffix(b"\n").unwrap_or(&root);
        let root = PathBuf::from(String::from_utf8(root.to_vec())?);
        ensure!(
            git(&root, &["ls-files", "--unmerged", "-z"], &[0])?.is_empty(),
            "Resolve merge conflicts before reviewing"
        );
        let head = String::from_utf8(git(
            &root,
            &["rev-parse", "--verify", "-q", "HEAD"],
            &[0, 1],
        )?)?
        .trim()
        .to_string();
        let untracked = paths(&git(
            &root,
            &["ls-files", "--others", "--exclude-standard", "-z"],
            &[0],
        )?)?;
        let tracked = if head.is_empty() {
            paths(&git(&root, &["ls-files", "--cached", "-z"], &[0])?)?
        } else {
            paths(&git(
                &root,
                &[
                    "diff",
                    "--no-ext-diff",
                    "--no-textconv",
                    "--no-renames",
                    "--no-relative",
                    "--ignore-submodules=none",
                    "--name-only",
                    "-z",
                    &head,
                    "--",
                ],
                &[0],
            )?)?
        };
        let mut files = Vec::new();
        let mut total = 0;
        for path in tracked.union(&untracked) {
            let is_new = head.is_empty() || untracked.contains(path);
            let mut args = vec!["diff"];
            args.extend(DIFF_OPTIONS);
            if is_new {
                // An unborn repository may have an indexed file deleted on disk.
                if std::fs::symlink_metadata(root.join(path))
                    .is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound)
                {
                    continue;
                }
                ensure!(
                    !root.join(path).is_dir(),
                    "Nested repository or directory cannot be reviewed: {path}"
                );
                args.extend(["--no-index", "--", "/dev/null", path]);
            } else {
                args.extend([&head, "--", path]);
            }
            let patch = git(&root, &args, if is_new { &[0, 1] } else { &[0] })?;
            if patch.is_empty() {
                continue;
            }
            total += patch.len();
            ensure!(
                total as u64 <= MAX_OUTPUT,
                "Review exceeds 32 MiB; reduce the working-tree changes"
            );
            let text = String::from_utf8_lossy(&patch);
            let status = if is_new || text.lines().any(|l| l.starts_with("new file mode ")) {
                'A'
            } else if text.lines().any(|l| l.starts_with("deleted file mode ")) {
                'D'
            } else {
                'M'
            };
            files.push(FileDiff {
                path: path.clone(),
                status,
                rows: parse_patch(&text)?,
                patch,
            });
        }
        Ok(Self { root, head, files })
    }

    pub fn unchanged(&self) -> Result<bool> {
        Ok(*self == Self::load(&self.root)?)
    }
}

pub fn parse_patch(patch: &str) -> Result<Vec<Row>> {
    let mut rows = Vec::new();
    let (mut old, mut new) = (0, 0);
    let (mut old_left, mut new_left) = (0, 0);
    for line in patch.split_terminator('\n') {
        if line.starts_with("@@ ") {
            ensure!(old_left == 0 && new_left == 0, "Incomplete diff hunk");
            let fields: Vec<_> = line.split_whitespace().collect();
            ensure!(
                fields.len() >= 4 && fields[3] == "@@",
                "Invalid diff hunk: {line}"
            );
            (old, old_left) = range(fields[1], '-')?;
            (new, new_left) = range(fields[2], '+')?;
            rows.push(Row {
                kind: Kind::Hunk,
                text: line.into(),
                old: None,
                new: None,
            });
        } else if old_left > 0 || new_left > 0 {
            let (kind, o, n) = match line.as_bytes().first() {
                Some(b' ') if old_left > 0 && new_left > 0 => {
                    old_left -= 1;
                    new_left -= 1;
                    old += 1;
                    new += 1;
                    (Kind::Context, Some(old - 1), Some(new - 1))
                }
                Some(b'-') if old_left > 0 => {
                    old_left -= 1;
                    old += 1;
                    (Kind::Delete, Some(old - 1), None)
                }
                Some(b'+') if new_left > 0 => {
                    new_left -= 1;
                    new += 1;
                    (Kind::Add, None, Some(new - 1))
                }
                Some(b'\\') => {
                    rows.push(Row {
                        kind: Kind::Meta,
                        text: line.into(),
                        old: None,
                        new: None,
                    });
                    continue;
                }
                _ => bail!("Invalid diff line: {line}"),
            };
            rows.push(Row {
                kind,
                text: line[1..].into(),
                old: o,
                new: n,
            });
        } else if !line.starts_with("diff --git ")
            && !line.starts_with("index ")
            && !line.starts_with("--- ")
            && !line.starts_with("+++ ")
        {
            rows.push(Row {
                kind: Kind::Meta,
                text: line.into(),
                old: None,
                new: None,
            });
        }
    }
    ensure!(old_left == 0 && new_left == 0, "Incomplete diff hunk");
    if rows.is_empty() {
        rows.push(Row {
            kind: Kind::Meta,
            text: "No textual changes".into(),
            old: None,
            new: None,
        });
    }
    Ok(rows)
}

fn range(s: &str, prefix: char) -> Result<(usize, usize)> {
    let s = s.strip_prefix(prefix).context("Invalid hunk range")?;
    let (start, count) = s.split_once(',').unwrap_or((s, "1"));
    Ok((start.parse()?, count.parse()?))
}

// Escape terminal control characters while leaving the underlying anchors exact.
pub fn display_text(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '\t' => "    ".into(),
            c if c.is_control() => c.escape_default().to_string(),
            c => c.to_string(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hunk_coordinates_and_header_like_content() {
        let rows = parse_patch("@@ -10,2 +20,3 @@ f\n context\n--- old\n+++ new\n+extra\n\\ No newline at end of file\n").unwrap();
        assert_eq!((rows[1].old, rows[1].new), (Some(10), Some(20)));
        assert_eq!(rows[2].kind, Kind::Delete);
        assert_eq!(rows[2].text, "-- old");
        assert_eq!(rows[4].new, Some(22));
        assert_eq!(rows[5].kind, Kind::Meta);
    }

    #[test]
    fn empty_ranges_and_missing_counts() {
        let r = parse_patch("@@ -0,0 +1 @@\n+x\n@@ -8 +9,0 @@\n-y\n").unwrap();
        assert_eq!(r[1].new, Some(1));
        assert_eq!(r[3].old, Some(8));
        assert!(parse_patch("@@ -1,2 +1 @@\n x\n").is_err());
    }

    #[test]
    fn paths_are_nul_delimited_and_controls_are_visible() {
        assert!(
            paths(b"space name\0line\nbreak\0")
                .unwrap()
                .contains("line\nbreak")
        );
        assert_eq!(display_text("x\x1b[31m\ty"), "x\\u{1b}[31m    y");
        assert!(paths(b"\xff\0").is_err());
    }
}
