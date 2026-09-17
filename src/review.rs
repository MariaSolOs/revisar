use crate::diff::{FileDiff, Kind, Snapshot};
use anyhow::{Result, bail, ensure};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Old,
    New,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Anchor {
    pub path: Option<String>,
    pub side: Option<Side>,
    pub start: Option<usize>,
    pub end: Option<usize>,
    pub excerpt: String,
}

impl Anchor {
    pub fn general() -> Self {
        Self {
            path: None,
            side: None,
            start: None,
            end: None,
            excerpt: String::new(),
        }
    }

    pub fn file(file: &FileDiff) -> Self {
        Self {
            path: Some(file.path.clone()),
            ..Self::general()
        }
    }

    pub fn lines(file: &FileDiff, a: usize, b: usize) -> Result<Self> {
        let rows = &file.rows[a.min(b)..=a.max(b)];
        ensure!(
            rows.iter().all(|r| r.old.is_some() || r.new.is_some()),
            "Select source lines within one hunk, not metadata or hunk headers"
        );
        let has_old = rows.iter().any(|r| r.kind == Kind::Delete);
        let has_new = rows.iter().any(|r| r.kind == Kind::Add);
        ensure!(
            !(has_old && has_new),
            "A range cannot mix deleted and added lines; comment on each side separately"
        );
        let side = if has_old { Side::Old } else { Side::New };
        let number = |r: &crate::diff::Row| if side == Side::Old { r.old } else { r.new };
        let Some(start) = number(&rows[0]) else {
            bail!("No source line at cursor")
        };
        let end = number(rows.last().unwrap()).unwrap();
        Ok(Self {
            path: Some(file.path.clone()),
            side: Some(side),
            start: Some(start),
            end: Some(end),
            excerpt: rows
                .iter()
                .map(|r| r.text.as_str())
                .collect::<Vec<_>>()
                .join("\n"),
        })
    }

    pub fn heading(&self) -> String {
        let Some(path) = &self.path else {
            return "General review comment".into();
        };
        // Debug quoting makes unusual path characters unambiguous to the agent.
        let mut s = format!("{path:?}");
        if let Some(start) = self.start {
            s.push_str(&format!(":{start}"));
            if let Some(end) = self.end.filter(|e| *e != start) {
                s.push_str(&format!("-{end}"));
            }
        }
        if let Some(side) = self.side {
            s.push_str(if side == Side::Old {
                " (old side, HEAD)"
            } else {
                " (new side, working tree)"
            });
        }
        s
    }
}

#[derive(Clone, Debug)]
pub struct Comment {
    pub anchor: Anchor,
    pub body: String,
}

pub fn markdown(snapshot: &Snapshot, comments: &[Comment], stale: bool) -> String {
    if comments.is_empty() {
        return String::new();
    }
    let mut s = format!(
        "I reviewed the working tree in {:?} with revisar and left {} comment(s).\n\nAddress each comment. Ask me if anything is ambiguous.\n\nBase HEAD: {}. The review covers the net staged, unstaged, and untracked changes, not just changes attributable to you.\n",
        snapshot.root,
        comments.len(),
        if snapshot.head.is_empty() {
            "unborn"
        } else {
            &snapshot.head
        }
    );
    if stale {
        s.push_str("\nWARNING: The working tree changed during this review. These anchors refer to the earlier snapshot. Locate the quoted code before editing; do not apply line numbers blindly.\n");
    }
    for (i, c) in comments.iter().enumerate() {
        s.push_str(&format!(
            "\n## {}. {}\n\n{}\n",
            i + 1,
            c.anchor.heading(),
            c.body.trim()
        ));
        if !c.anchor.excerpt.is_empty() {
            // Code can itself contain Markdown fences; choose a longer fence.
            let longest = c
                .anchor
                .excerpt
                .split(|c| c != '`')
                .map(str::len)
                .max()
                .unwrap_or(0);
            let fence = "`".repeat(3.max(longest + 1));
            s.push_str(&format!(
                "\nReviewed code:\n{fence}\n{}\n{fence}\n",
                c.anchor.excerpt
            ));
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diff::parse_patch;

    pub fn fixture() -> FileDiff {
        FileDiff {
            path: "src/a.rs".into(),
            status: 'M',
            patch: vec![],
            rows: parse_patch("@@ -2,3 +2,3 @@\n context\n-old\n+new\n tail\n").unwrap(),
        }
    }

    #[test]
    fn ranges_keep_correct_side_and_reject_mixed_selections() {
        let f = fixture();
        let a = Anchor::lines(&f, 2, 1).unwrap();
        assert_eq!(
            (a.start, a.end, a.side),
            (Some(2), Some(3), Some(Side::Old))
        );
        assert_eq!(Anchor::lines(&f, 3, 4).unwrap().side, Some(Side::New));
        assert!(Anchor::lines(&f, 2, 3).is_err());
        assert!(Anchor::lines(&f, 0, 1).is_err());
    }

    #[test]
    fn feedback_has_coordinates_excerpt_and_stale_warning() {
        let f = fixture();
        let snapshot = Snapshot {
            root: "/repo".into(),
            head: "abc123".into(),
            files: vec![f.clone()],
        };
        assert!(markdown(&snapshot, &[], false).is_empty());
        let comments = vec![Comment {
            anchor: Anchor::lines(&f, 2, 2).unwrap(),
            body: "Fix it\nplease".into(),
        }];
        let m = markdown(&snapshot, &comments, true);
        assert!(m.contains("\"src/a.rs\":3 (old side, HEAD)"));
        assert!(m.contains("WARNING:"));
        assert!(m.contains("```\nold\n```"));
        assert!(m.contains("Fix it\nplease"));
    }
}
