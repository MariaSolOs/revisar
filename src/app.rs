use crate::{
    diff::{Kind, Snapshot},
    editor::Editor,
    review::{Anchor, Comment, Side},
};
use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

#[derive(Clone, Copy, Default)]
pub struct View {
    pub row: usize,
    pub top: usize,
    pub left: usize,
    // One-shot request, applied with the actual viewport height on the next draw.
    pub center: bool,
}

pub struct Draft {
    pub anchor: Anchor,
    pub editing: Option<usize>,
    pub editor: Editor,
}

#[derive(Clone, Copy)]
pub enum Confirmation {
    Send,
    Stale,
    Discard,
    Delete(usize),
}

pub enum Mode {
    Normal,
    Search(Editor),
    Comment(Draft),
    Confirm(Confirmation),
    Help(usize),
}

#[derive(Debug, PartialEq, Eq)]
pub enum Action {
    Continue,
    Send,
    SendStale,
    Cancel,
}

pub struct App {
    pub snapshot: Snapshot,
    pub file: usize,
    pub views: Vec<View>,
    pub comments: Vec<Comment>,
    pub mode: Mode,
    pub files_focused: bool,
    pub selection: Option<usize>,
    pub summary: bool,
    pub comment: usize,
    pub summary_scroll: usize,
    pub query: String,
    pub matches: Vec<(usize, usize)>,
    pub message: String,
    pub height: usize,
}

impl App {
    pub fn new(snapshot: Snapshot) -> Self {
        let count = snapshot.files.len();
        Self {
            snapshot,
            file: 0,
            views: vec![View::default(); count],
            comments: vec![],
            mode: Mode::Normal,
            files_focused: false,
            selection: None,
            summary: false,
            comment: 0,
            summary_scroll: 0,
            query: String::new(),
            matches: vec![],
            message: String::new(),
            height: 20,
        }
    }

    pub fn event(&mut self, event: Event) -> Action {
        match event {
            Event::Paste(text) => match &mut self.mode {
                Mode::Comment(d) => d.editor.insert(&text),
                Mode::Search(e) => e.insert(&text.replace(['\r', '\n'], " ")),
                _ => (),
            },
            Event::Key(key) if key.kind != KeyEventKind::Release => return self.key(key),
            _ => (),
        }
        Action::Continue
    }

    fn key(&mut self, key: KeyEvent) -> Action {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match &mut self.mode {
            Mode::Comment(draft) => {
                if key.code == KeyCode::Esc || (ctrl && key.code == KeyCode::Char('c')) {
                    self.mode = Mode::Normal;
                } else if (ctrl && key.code == KeyCode::Char('s'))
                    || (key.code == KeyCode::Enter && !key.modifiers.contains(KeyModifiers::SHIFT))
                {
                    let body = draft.editor.text().trim().to_string();
                    if body.is_empty() {
                        self.message = "A comment cannot be empty".into();
                    } else {
                        let comment = Comment {
                            anchor: draft.anchor.clone(),
                            body,
                        };
                        if let Some(i) = draft.editing {
                            self.comments[i] = comment;
                        } else {
                            self.comments.push(comment);
                            self.comment = self.comments.len() - 1;
                        }
                        self.mode = Mode::Normal;
                        self.selection = None;
                        self.message = "Comment added to this review. S sends all feedback.".into();
                    }
                } else {
                    draft.editor.input(key);
                }
                return Action::Continue;
            }
            Mode::Search(editor) => {
                if key.code == KeyCode::Esc || (ctrl && key.code == KeyCode::Char('c')) {
                    self.mode = Mode::Normal;
                } else if key.code == KeyCode::Enter {
                    self.query = editor.text();
                    self.mode = Mode::Normal;
                    self.matches.clear();
                    if !self.query.is_empty() {
                        let q = self.query.to_lowercase();
                        for (f, file) in self.snapshot.files.iter().enumerate() {
                            for (r, row) in file.rows.iter().enumerate() {
                                if row.text.to_lowercase().contains(&q) {
                                    self.matches.push((f, r));
                                }
                            }
                        }
                    }
                    self.next_match(false);
                } else {
                    editor.input(key);
                }
                return Action::Continue;
            }
            Mode::Confirm(confirmation) => {
                let confirmation = *confirmation;
                if matches!(key.code, KeyCode::Char('y') | KeyCode::Enter) {
                    self.mode = Mode::Normal;
                    return match confirmation {
                        Confirmation::Send => Action::Send,
                        Confirmation::Stale => Action::SendStale,
                        Confirmation::Discard => Action::Cancel,
                        Confirmation::Delete(i) => {
                            self.comments.remove(i);
                            self.comment = self.comment.min(self.comments.len().saturating_sub(1));
                            self.summary_scroll = 0;
                            Action::Continue
                        }
                    };
                }
                if matches!(
                    key.code,
                    KeyCode::Esc | KeyCode::Char('n') | KeyCode::Char('q')
                ) || ctrl
                {
                    self.mode = Mode::Normal;
                }
                return Action::Continue;
            }
            Mode::Help(scroll) => {
                match key.code {
                    KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('?') => {
                        self.mode = Mode::Normal
                    }
                    KeyCode::Down | KeyCode::Char('j') => *scroll = scroll.saturating_add(1),
                    KeyCode::Up | KeyCode::Char('k') => *scroll = scroll.saturating_sub(1),
                    KeyCode::Char('d') if ctrl => *scroll = scroll.saturating_add(10),
                    KeyCode::Char('u') if ctrl => *scroll = scroll.saturating_sub(10),
                    KeyCode::Char('g') => *scroll = 0,
                    KeyCode::Char('G') => *scroll = usize::MAX,
                    _ => (),
                }
                return Action::Continue;
            }
            Mode::Normal => (),
        }
        self.message.clear();
        if ctrl {
            match key.code {
                KeyCode::Char('c') => return self.quit(),
                KeyCode::Char('d') => self.page(1),
                KeyCode::Char('u') => self.page(-1),
                _ => (),
            }
            return Action::Continue;
        }
        match key.code {
            KeyCode::Char('q') => return self.quit(),
            KeyCode::Char('?') => self.mode = Mode::Help(0),
            KeyCode::Char('S') => {
                if self.comments.is_empty() {
                    self.message = "No comments to send. q closes without feedback.".into();
                } else {
                    self.mode = Mode::Confirm(Confirmation::Send);
                }
            }
            KeyCode::Char('s') => {
                self.summary = !self.summary;
                self.summary_scroll = 0;
                self.files_focused = false;
                self.selection = None;
            }
            KeyCode::Tab | KeyCode::BackTab => {
                self.files_focused = !self.files_focused;
                self.selection = None;
            }
            KeyCode::Esc => {
                self.selection = None;
                self.summary = false;
                self.query.clear();
                self.matches.clear();
            }
            KeyCode::Char('j') | KeyCode::Down => self.move_by(1),
            KeyCode::Char('k') | KeyCode::Up => self.move_by(-1),
            KeyCode::PageDown => self.page(2),
            KeyCode::PageUp => self.page(-2),
            KeyCode::Char('g') | KeyCode::Home => self.move_by(isize::MIN),
            KeyCode::Char('G') | KeyCode::End => self.move_by(isize::MAX),
            KeyCode::Char('{') => self.change_file(self.file.saturating_sub(1)),
            KeyCode::Char('}') => self.change_file(self.file.saturating_add(1)),
            KeyCode::Char('a') => self.draft(Anchor::general()),
            KeyCode::Char('i') => {
                if let Some(i) = self.current_comment() {
                    let c = &self.comments[i];
                    self.mode = Mode::Comment(Draft {
                        anchor: c.anchor.clone(),
                        editing: Some(i),
                        editor: Editor::new(&c.body),
                    });
                } else {
                    self.message = "No comment here. s opens the comment summary.".into();
                }
            }
            KeyCode::Char('d') => {
                if let Some(i) = self.current_comment() {
                    self.mode = Mode::Confirm(Confirmation::Delete(i));
                }
            }
            KeyCode::Enter if self.files_focused => {
                self.files_focused = false;
                self.summary = false;
            }
            KeyCode::Enter if self.summary => self.jump_comment(),
            KeyCode::Char('/') => {
                self.selection = None;
                self.mode = Mode::Search(Editor::new(&self.query));
            }
            KeyCode::Char('n') => self.next_match(false),
            KeyCode::Char('N') => self.next_match(true),
            _ if self.snapshot.files.is_empty() => (),
            KeyCode::Char('C') => self.draft(Anchor::file(&self.snapshot.files[self.file])),
            _ if self.summary || self.files_focused => (),
            KeyCode::Char('h') | KeyCode::Left => {
                self.views[self.file].left = self.views[self.file].left.saturating_sub(4)
            }
            KeyCode::Char('l') | KeyCode::Right => {
                self.views[self.file].left = (self.views[self.file].left + 4).min(100_000)
            }
            KeyCode::Char('[') => self.hunk(false),
            KeyCode::Char(']') => self.hunk(true),
            KeyCode::Char('v') | KeyCode::Char('V') => {
                self.selection = if self.selection.is_some() {
                    None
                } else {
                    Some(self.views[self.file].row)
                };
            }
            KeyCode::Char('c') | KeyCode::Enter => {
                let row = self.views[self.file].row;
                let file = &self.snapshot.files[self.file];
                if key.code == KeyCode::Enter && self.selection.is_none() {
                    return Action::Continue;
                }
                if self.selection.is_none()
                    && file.rows[row].old.is_none()
                    && file.rows[row].new.is_none()
                {
                    self.draft(Anchor::file(file));
                } else {
                    match Anchor::lines(file, self.selection.unwrap_or(row), row) {
                        Ok(anchor) => self.draft(anchor),
                        Err(e) => self.message = e.to_string(),
                    }
                }
            }
            _ => (),
        }
        Action::Continue
    }

    fn quit(&mut self) -> Action {
        if self.comments.is_empty() {
            Action::Cancel
        } else {
            self.mode = Mode::Confirm(Confirmation::Discard);
            Action::Continue
        }
    }

    fn draft(&mut self, anchor: Anchor) {
        self.mode = Mode::Comment(Draft {
            anchor,
            editing: None,
            editor: Editor::default(),
        });
    }

    fn change_file(&mut self, file: usize) {
        self.file = file.min(self.snapshot.files.len().saturating_sub(1));
        self.selection = None;
    }

    fn move_by(&mut self, delta: isize) {
        if self.files_focused {
            self.change_file(self.file.saturating_add_signed(delta));
        } else if self.summary {
            self.comment = self
                .comment
                .saturating_add_signed(delta)
                .min(self.comments.len().saturating_sub(1));
            self.summary_scroll = 0;
        } else if let Some(view) = self.views.get_mut(self.file) {
            view.row = view
                .row
                .saturating_add_signed(delta)
                .min(self.snapshot.files[self.file].rows.len().saturating_sub(1));
        }
    }

    fn page(&mut self, direction: isize) {
        let amount = direction * (self.height / 2).max(1) as isize;
        if self.summary && !self.files_focused {
            self.summary_scroll = self.summary_scroll.saturating_add_signed(amount);
        } else {
            self.move_by(amount);
            if !self.files_focused
                && let Some(view) = self.views.get_mut(self.file)
            {
                view.center = true;
            }
        }
    }

    fn hunk(&mut self, forward: bool) {
        let rows = &self.snapshot.files[self.file].rows;
        let row = self.views[self.file].row;
        let target = if forward {
            rows.iter()
                .enumerate()
                .find(|(i, r)| *i > row && r.kind == Kind::Hunk)
        } else {
            rows.iter()
                .enumerate()
                .rev()
                .find(|(i, r)| *i < row && r.kind == Kind::Hunk)
        };
        if let Some((i, _)) = target {
            self.views[self.file].row = i;
            self.views[self.file].center = true;
        }
    }

    fn next_match(&mut self, backwards: bool) {
        let current = (self.file, self.views.get(self.file).map_or(0, |v| v.row));
        let target = if backwards {
            self.matches
                .iter()
                .rev()
                .find(|m| **m < current)
                .or_else(|| self.matches.last())
        } else {
            self.matches
                .iter()
                .find(|m| **m > current)
                .or_else(|| self.matches.first())
        }
        .copied();
        if let Some((f, r)) = target {
            self.change_file(f);
            self.views[f].row = r;
            self.views[f].left = 0;
            self.views[f].center = true;
            self.summary = false;
            self.files_focused = false;
            self.message = format!("{} matching lines", self.matches.len());
        } else {
            self.message = "No matching lines".into();
        }
    }

    pub fn comment_on_row(&self, comment: &Comment, row: usize) -> bool {
        let Some(file) = self.snapshot.files.get(self.file) else {
            return false;
        };
        if comment.anchor.path.as_deref() != Some(&file.path) {
            return false;
        }
        let r = &file.rows[row];
        if let Some(start) = comment.anchor.start {
            let n = if comment.anchor.side == Some(Side::Old) {
                r.old
            } else {
                r.new
            };
            n.is_some_and(|n| n >= start && n <= comment.anchor.end.unwrap_or(start))
        } else {
            r.old.is_none() && r.new.is_none()
        }
    }

    pub fn current_comment(&self) -> Option<usize> {
        if self.summary {
            return (self.comment < self.comments.len()).then_some(self.comment);
        }
        let row = self.views.get(self.file)?.row;
        self.comments
            .iter()
            .position(|c| self.comment_on_row(c, row))
    }

    fn jump_comment(&mut self) {
        let Some(comment) = self.comments.get(self.comment) else {
            return;
        };
        let anchor = comment.anchor.clone();
        if let Some(f) = self
            .snapshot
            .files
            .iter()
            .position(|f| Some(&f.path) == anchor.path.as_ref())
        {
            self.change_file(f);
            if let Some(start) = anchor.start {
                self.views[f].row = self.snapshot.files[f]
                    .rows
                    .iter()
                    .position(|r| {
                        (if anchor.side == Some(Side::Old) {
                            r.old
                        } else {
                            r.new
                        }) == Some(start)
                    })
                    .unwrap_or(0);
            } else {
                self.views[f].row = 0;
            }
            self.summary = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn app() -> App {
        App::new(Snapshot {
            root: "/repo".into(),
            head: "abc".into(),
            files: vec![crate::diff::FileDiff {
                path: "a.rs".into(),
                status: 'M',
                patch: vec![],
                rows: crate::diff::parse_patch("@@ -1 +1 @@\n-old\n+new\n").unwrap(),
            }],
        })
    }
    fn press(a: &mut App, key: char) -> Action {
        a.event(Event::Key(KeyEvent::new(
            KeyCode::Char(key),
            KeyModifiers::NONE,
        )))
    }
    #[test]
    fn comment_submit_cancel_and_delete_are_explicit() {
        let mut a = app();
        press(&mut a, 'j');
        press(&mut a, 'c');
        a.event(Event::Paste("Please fix".into()));
        a.event(Event::Key(KeyCode::Enter.into()));
        assert_eq!(a.comments.len(), 1);
        assert_eq!(press(&mut a, 'q'), Action::Continue);
        assert!(matches!(a.mode, Mode::Confirm(Confirmation::Discard)));
        press(&mut a, 'n');
        press(&mut a, 'S');
        assert_eq!(press(&mut a, 'y'), Action::Send);
        press(&mut a, 's');
        press(&mut a, 'd');
        press(&mut a, 'y');
        assert!(a.comments.is_empty());
    }
    #[test]
    fn paging_other_panels_does_not_request_diff_centering() {
        let mut a = app();
        a.files_focused = true;
        a.page(1);
        assert!(!a.views[0].center);
        assert_eq!(a.views[0].row, 0);

        a.files_focused = false;
        a.summary = true;
        a.page(1);
        assert_eq!(a.summary_scroll, 10);
        assert!(!a.views[0].center);
        assert_eq!(a.views[0].row, 0);
    }

    #[test]
    fn search_wraps_and_empty_tree_is_safe() {
        let mut a = app();
        press(&mut a, '/');
        a.event(Event::Paste("NEW".into()));
        a.event(Event::Key(KeyCode::Enter.into()));
        assert_eq!(a.views[0].row, 2);
        press(&mut a, 'n');
        assert_eq!(a.views[0].row, 2);
        let mut a = App::new(Snapshot {
            root: "/repo".into(),
            head: "".into(),
            files: vec![],
        });
        for c in "jkgG{}[]vcrCSsdi/nN".chars() {
            press(&mut a, c);
        }
    }
}
