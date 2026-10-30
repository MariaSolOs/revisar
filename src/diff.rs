use anyhow::{Context, Result, bail, ensure};
use std::{
    collections::{BTreeMap, BTreeSet},
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
    // Commit the working tree is diffed against: HEAD, or, when the working
    // tree is clean, its merge base with the target branch it diverged from.
    pub base: String,
    // Short target ref (e.g. "origin/main") when base is a merge base.
    pub target: Option<String>,
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
            // Keep patch headers predictable for splitting batched diffs.
            "-c",
            "core.quotePath=false",
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

fn line(bytes: Vec<u8>) -> Result<String> {
    Ok(String::from_utf8(bytes)?.trim().to_string())
}

// Review a branch against where it forked from the default branch, so its
// committed changes stay visible even when the working tree is clean.
fn branch_base(root: &Path, head: &str) -> Result<Option<(String, String)>> {
    let mut candidates = Vec::new();
    let origin = line(git(
        root,
        &["symbolic-ref", "-q", "refs/remotes/origin/HEAD"],
        &[0, 1],
    )?)?;
    if !origin.is_empty() {
        candidates.push(origin);
    }
    candidates.extend(["refs/heads/main".into(), "refs/heads/master".into()]);
    for full in candidates {
        let spec = format!("{full}^{{commit}}");
        if line(git(root, &["rev-parse", "--verify", "-q", &spec], &[0, 1])?)?.is_empty() {
            continue;
        }
        // Unrelated histories have no merge base: fall back to HEAD.
        let base = line(git(root, &["merge-base", head, &full], &[0, 1])?)?;
        if base.is_empty() || base == head {
            return Ok(None);
        }
        let short = full
            .strip_prefix("refs/remotes/")
            .or_else(|| full.strip_prefix("refs/heads/"))
            .unwrap_or(&full)
            .to_string();
        return Ok(Some((base, short)));
    }
    Ok(None)
}

// The `diff --git` header Git writes for an unrenamed path (core.quotePath=false).
fn header(path: &str) -> Vec<u8> {
    let quote = path
        .bytes()
        .any(|b| b == b'"' || b == b'\\' || b < 0x20 || b == 0x7f);
    let side = |prefix: &str| {
        let mut out = Vec::new();
        if !quote {
            out.extend(prefix.bytes().chain(path.bytes()));
            return out;
        }
        out.push(b'"');
        for b in prefix.bytes().chain(path.bytes()) {
            match b {
                0x07 => out.extend(b"\\a"),
                0x08 => out.extend(b"\\b"),
                b'\t' => out.extend(b"\\t"),
                b'\n' => out.extend(b"\\n"),
                0x0b => out.extend(b"\\v"),
                0x0c => out.extend(b"\\f"),
                b'\r' => out.extend(b"\\r"),
                b'"' => out.extend(b"\\\""),
                b'\\' => out.extend(b"\\\\"),
                b if b < 0x20 || b == 0x7f => out.extend(format!("\\{b:03o}").bytes()),
                b => out.push(b),
            }
        }
        out.push(b'"');
        out
    };
    let mut out = b"diff --git ".to_vec();
    out.extend(side("a/"));
    out.push(b' ');
    out.extend(side("b/"));
    out
}

// Split one multi-file patch at its `diff --git` headers. Hunk lines always
// start with a prefix character, so a header can't appear inside a hunk.
fn split_patch(bytes: &[u8], paths: &BTreeSet<String>) -> Result<BTreeMap<String, Vec<u8>>> {
    let mut starts = Vec::new();
    let mut at = 0;
    while at < bytes.len() {
        if bytes[at..].starts_with(b"diff --git ") {
            starts.push(at);
        }
        at = match bytes[at..].iter().position(|b| *b == b'\n') {
            Some(i) => at + i + 1,
            None => bytes.len(),
        };
    }
    ensure!(
        starts.first().is_none_or(|s| *s == 0) && starts.len() == paths.len(),
        "Unexpected batched diff output"
    );
    starts.push(bytes.len());
    let mut files = BTreeMap::new();
    for (path, w) in paths.iter().zip(starts.windows(2)) {
        let patch = &bytes[w[0]..w[1]];
        let first = patch.split(|b| *b == b'\n').next().unwrap_or_default();
        ensure!(
            first == header(path).as_slice(),
            "Unexpected diff header for {path}"
        );
        files.insert(path.clone(), patch.to_vec());
    }
    Ok(files)
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
        // The Pi dotfiles fallback reviews a home worktree: never enumerate its
        // unrelated untracked files. Keep this mode for send-time reloads too.
        Self::load_with_untracked(
            cwd,
            std::env::var_os("REVISAR_TRACKED_ONLY").as_deref() != Some(std::ffi::OsStr::new("1")),
        )
    }

    fn load_with_untracked(cwd: &Path, include_untracked: bool) -> Result<Self> {
        let root = git(cwd, &["rev-parse", "--show-toplevel"], &[0])?;
        // Strip only Git's terminating LF, not whitespace belonging to the path.
        let root = root.strip_suffix(b"\n").unwrap_or(&root);
        let root = PathBuf::from(String::from_utf8(root.to_vec())?);
        ensure!(
            git(&root, &["ls-files", "--unmerged", "-z"], &[0])?.is_empty(),
            "Resolve merge conflicts before reviewing"
        );
        let mut head = line(git(
            &root,
            &["rev-parse", "--verify", "-q", "HEAD"],
            &[0, 1],
        )?)?;
        let untracked = if include_untracked {
            paths(&git(
                &root,
                &["ls-files", "--others", "--exclude-standard", "-z"],
                &[0],
            )?)?
        } else {
            BTreeSet::new()
        };
        // Uncommitted changes take priority. Only a clean working tree falls back
        // to the branch's committed changes against its merge base.
        let mut files = Self::files(&root, &head, &untracked)?;
        let mut target = None;
        if files.is_empty()
            && !head.is_empty()
            && let Some((base, short)) = branch_base(&root, &head)?
        {
            files = Self::files(&root, &base, &untracked)?;
            head = base;
            target = Some(short);
        }
        Ok(Self {
            root,
            base: head,
            target,
            files,
        })
    }

    fn files(root: &Path, head: &str, untracked: &BTreeSet<String>) -> Result<Vec<FileDiff>> {
        let tracked = if head.is_empty() {
            paths(&git(root, &["ls-files", "--cached", "-z"], &[0])?)?
        } else {
            paths(&git(
                root,
                &[
                    "diff",
                    "--no-ext-diff",
                    "--no-textconv",
                    "--no-renames",
                    "--no-relative",
                    "--ignore-submodules=none",
                    "--name-only",
                    "-z",
                    head,
                    "--",
                ],
                &[0],
            )?)?
        };
        // One diff for all tracked paths: a process per file is far too slow
        // for long branches in large repositories.
        let mut batched = if head.is_empty() {
            BTreeMap::new()
        } else {
            let mut args = vec!["diff"];
            args.extend(DIFF_OPTIONS);
            args.extend([head, "--"]);
            split_patch(&git(root, &args, &[0])?, &tracked)?
        };
        let mut files = Vec::new();
        let mut total = 0;
        for path in tracked.union(untracked) {
            let is_new = head.is_empty() || untracked.contains(path);
            let patch = if is_new {
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
                let mut args = vec!["diff"];
                args.extend(DIFF_OPTIONS);
                args.extend(["--no-index", "--", "/dev/null", path]);
                git(root, &args, &[0, 1])?
            } else {
                batched.remove(path).unwrap_or_default()
            };
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
        Ok(files)
    }

    pub fn label(&self) -> String {
        match &self.target {
            Some(target) => format!("branch vs {target}"),
            None => "working tree".into(),
        }
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
    fn tracked_only_omits_untracked_files() {
        let dir = tempfile::Builder::new()
            .prefix("tracked-only-")
            .tempdir_in("target")
            .unwrap();
        let repo = git2::Repository::init(dir.path()).unwrap();
        std::fs::write(dir.path().join("tracked"), "config\n").unwrap();
        std::fs::write(dir.path().join("private"), "unrelated\n").unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(Path::new("tracked")).unwrap();
        index.write().unwrap();
        let snapshot = Snapshot::load_with_untracked(dir.path(), false).unwrap();
        assert_eq!(snapshot.files.len(), 1);
        assert_eq!(snapshot.files[0].path, "tracked");
        assert_eq!(
            Snapshot::load_with_untracked(dir.path(), true)
                .unwrap()
                .files
                .len(),
            2
        );
    }

    #[test]
    fn headers_match_git_quoting() {
        assert_eq!(
            header("a b/界"),
            b"diff --git a/a b/\xe7\x95\x8c b/a b/\xe7\x95\x8c"
        );
        assert_eq!(
            header("x\n\"\\\x01"),
            b"diff --git \"a/x\\n\\\"\\\\\\001\" \"b/x\\n\\\"\\\\\\001\""
        );
        let paths: BTreeSet<_> = ["a".to_string(), "b".into()].into();
        let patch = b"diff --git a/a b/a\n@@ -1 +1 @@\n-diff --git x\n+y\ndiff --git a/b b/b\n";
        let split = split_patch(patch, &paths).unwrap();
        assert!(split["a"].ends_with(b"+y\n"));
        assert_eq!(split["b"], b"diff --git a/b b/b\n");
        assert!(split_patch(b"diff --git a/b b/b\n", &["a".to_string()].into()).is_err());
    }

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
