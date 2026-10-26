use crate::wrap::word_ranges;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use unicode_width::UnicodeWidthChar;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum VimMode {
    #[default]
    Insert,
    Normal,
}

// Text covered by an operator: a character range, or whole lines given by
// the first line's start and the last line's end.
enum Span {
    Chars(usize, usize),
    Lines(usize, usize),
}

#[derive(Default)]
pub struct Editor {
    pub chars: Vec<char>,
    pub cursor: usize,
    // Vim state, used only through `vim`, `escape`, and `paste`.
    pub mode: VimMode,
    pending: Option<char>,
    undo: Vec<(Vec<char>, usize)>,
}

impl Editor {
    pub fn new(s: &str) -> Self {
        let chars: Vec<_> = s.chars().collect();
        Self {
            cursor: chars.len(),
            chars,
            ..Self::default()
        }
    }

    // Start in Vim Normal mode with the cursor at `cursor`.
    pub fn normal(s: &str, cursor: usize) -> Self {
        let mut editor = Self::new(s);
        editor.mode = VimMode::Normal;
        editor.cursor = cursor.min(editor.chars.len());
        editor.clamp();
        editor
    }

    pub fn text(&self) -> String {
        self.chars.iter().collect()
    }

    pub fn insert(&mut self, s: &str) {
        let chars: Vec<_> = s
            .replace("\r\n", "\n")
            .replace('\r', "\n")
            .replace('\t', "    ")
            .chars()
            .filter(|c| !c.is_control() || *c == '\n')
            .collect();
        let count = chars.len();
        self.chars.splice(self.cursor..self.cursor, chars);
        self.cursor += count;
    }

    fn line_start(&self, i: usize) -> usize {
        self.chars[..i]
            .iter()
            .rposition(|c| *c == '\n')
            .map_or(0, |i| i + 1)
    }

    fn line_end(&self, i: usize) -> usize {
        self.chars[i..]
            .iter()
            .position(|c| *c == '\n')
            .map_or(self.chars.len(), |p| i + p)
    }

    fn home(&self) -> usize {
        self.line_start(self.cursor)
    }

    fn end(&self) -> usize {
        self.line_end(self.cursor)
    }

    // Vim Normal-mode cursors rest on a character, never after a line's end.
    fn clamp(&mut self) {
        if self.mode == VimMode::Normal && self.cursor == self.end() && self.cursor > self.home() {
            self.cursor -= 1;
        }
    }

    fn checkpoint(&mut self) {
        self.undo.push((self.chars.clone(), self.cursor));
    }

    fn insert_mode(&mut self) {
        self.checkpoint();
        self.mode = VimMode::Insert;
    }

    /// Returns false when Esc has nothing left to leave, i.e. plain Normal mode.
    pub fn escape(&mut self) -> bool {
        if self.mode == VimMode::Insert {
            self.mode = VimMode::Normal;
            // An insert session that changed nothing is not an undo step.
            if self
                .undo
                .last()
                .is_some_and(|(chars, _)| *chars == self.chars)
            {
                self.undo.pop();
            }
            if self.cursor > self.home() {
                self.cursor -= 1;
            }
            true
        } else {
            self.pending.take().is_some()
        }
    }

    pub fn paste(&mut self, s: &str) {
        if self.mode == VimMode::Normal {
            self.pending = None;
            self.checkpoint();
        }
        self.insert(s);
        self.clamp();
    }

    pub fn vim(&mut self, key: KeyEvent) {
        if self.mode == VimMode::Insert {
            return self.input(key);
        }
        let pending = self.pending.take();
        if key
            .modifiers
            .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
        {
            return;
        }
        if let Some(op) = pending {
            if let Some(span) = self.span(op, key.code) {
                self.operate(op, span);
            }
            return self.clamp();
        }
        match key.code {
            KeyCode::Char(op @ ('d' | 'c')) => self.pending = Some(op),
            KeyCode::Char('h') | KeyCode::Left | KeyCode::Backspace => {
                if self.cursor > self.home() {
                    self.cursor -= 1;
                }
            }
            KeyCode::Char('l') | KeyCode::Right => {
                if self.cursor + 1 < self.end() {
                    self.cursor += 1;
                }
            }
            KeyCode::Char('j') => self.input(KeyCode::Down.into()),
            KeyCode::Char('k') => self.input(KeyCode::Up.into()),
            KeyCode::Up | KeyCode::Down => self.input(key),
            KeyCode::Char('0') | KeyCode::Home => self.cursor = self.home(),
            KeyCode::Char('$') | KeyCode::End => self.cursor = self.end(),
            KeyCode::Char('w') => self.cursor = self.next_word(self.cursor),
            KeyCode::Char('b') => self.cursor = self.prev_word(self.cursor),
            KeyCode::Char('i') => self.insert_mode(),
            KeyCode::Char('a') => {
                self.insert_mode();
                self.cursor = (self.cursor + 1).min(self.end());
            }
            KeyCode::Char('o') => {
                self.insert_mode();
                self.cursor = self.end();
                self.insert("\n");
            }
            KeyCode::Char('O') => {
                self.insert_mode();
                self.cursor = self.home();
                self.insert("\n");
                self.cursor -= 1;
            }
            KeyCode::Char('x') | KeyCode::Delete => {
                if self.chars.get(self.cursor).is_some_and(|c| *c != '\n') {
                    self.checkpoint();
                    self.chars.remove(self.cursor);
                }
            }
            KeyCode::Char('u') => {
                if let Some((chars, cursor)) = self.undo.pop() {
                    self.chars = chars;
                    self.cursor = cursor.min(self.chars.len());
                }
            }
            _ => (),
        }
        self.clamp();
    }

    // The text an operator covers for a motion, or None when the motion fails.
    fn span(&self, op: char, code: KeyCode) -> Option<Span> {
        let (c, n) = (self.cursor, self.chars.len());
        let (home, end) = (self.home(), self.end());
        Some(match code {
            KeyCode::Char(k) if k == op => Span::Lines(home, end),
            KeyCode::Char('h') | KeyCode::Left | KeyCode::Backspace if c > home => {
                Span::Chars(c - 1, c)
            }
            KeyCode::Char('l') | KeyCode::Right => Span::Chars(c, (c + 1).min(end)),
            KeyCode::Char('0') | KeyCode::Home => Span::Chars(home, c),
            KeyCode::Char('$') | KeyCode::End => Span::Chars(c, end),
            // On an empty line, `w` becomes linewise.
            KeyCode::Char('w') if c == end => Span::Lines(home, end),
            // Vim's `cw` inside a word changes only to the end of that word.
            KeyCode::Char('w') if op == 'c' && !self.chars[c].is_whitespace() => {
                let kind = class(self.chars[c]);
                let len = self.chars[c..end]
                    .iter()
                    .take_while(|ch| class(**ch) == kind)
                    .count();
                Span::Chars(c, c + len)
            }
            // `dw` on a line's last word stops at the line's end.
            KeyCode::Char('w') => Span::Chars(c, self.next_word(c).min(end)),
            KeyCode::Char('b') if c > 0 => {
                let start = self.prev_word(c);
                if c > home {
                    Span::Chars(start, c)
                } else {
                    // An exclusive motion ending at column 0 ends at the previous
                    // line's end, and is linewise if it starts in leading blanks.
                    let line = self.line_start(start);
                    if self.chars[line..start]
                        .iter()
                        .all(|ch| matches!(ch, ' ' | '\t'))
                    {
                        Span::Lines(line, c - 1)
                    } else {
                        Span::Chars(start, c - 1)
                    }
                }
            }
            KeyCode::Char('j') | KeyCode::Down if end < n => {
                Span::Lines(home, self.line_end(end + 1))
            }
            KeyCode::Char('k') | KeyCode::Up if home > 0 => {
                Span::Lines(self.line_start(home - 1), end)
            }
            _ => return None,
        })
    }

    // `d` deletes; `c` deletes and enters Insert mode as one undo step.
    fn operate(&mut self, op: char, span: Span) {
        self.checkpoint();
        match span {
            Span::Chars(start, end) => {
                self.chars.drain(start..end);
                self.cursor = start;
            }
            // `c` keeps one empty line to type into.
            Span::Lines(home, end) if op == 'c' => {
                self.chars.drain(home..end);
                self.cursor = home;
            }
            Span::Lines(home, end) => self.delete_lines(home, end),
        }
        if op == 'c' {
            self.mode = VimMode::Insert;
        } else if self
            .undo
            .last()
            .is_some_and(|(chars, _)| *chars == self.chars)
        {
            self.undo.pop();
        }
    }

    fn delete_lines(&mut self, home: usize, end: usize) {
        if end < self.chars.len() {
            self.chars.drain(home..=end);
            self.cursor = home;
        } else if home > 0 {
            // The last line takes its preceding newline with it.
            self.chars.drain(home - 1..end);
            self.cursor = self.line_start(home - 1);
        } else {
            self.chars.clear();
            self.cursor = 0;
        }
        let end = self.end();
        while self.cursor < end && matches!(self.chars[self.cursor], ' ' | '\t') {
            self.cursor += 1;
        }
    }

    fn empty_line(&self, i: usize) -> bool {
        self.chars.get(i).is_none_or(|c| *c == '\n') && (i == 0 || self.chars[i - 1] == '\n')
    }

    // Vim `w`: words are runs of keyword characters or of other punctuation;
    // an empty line also counts as a word.
    fn next_word(&self, mut i: usize) -> usize {
        let n = self.chars.len();
        if i < n && !self.chars[i].is_whitespace() {
            let kind = class(self.chars[i]);
            while i < n && class(self.chars[i]) == kind {
                i += 1;
            }
        }
        while i < n && self.chars[i].is_whitespace() {
            i += 1;
            if self.chars[i - 1] == '\n' && self.empty_line(i) {
                break;
            }
        }
        i
    }

    fn prev_word(&self, i: usize) -> usize {
        if i == 0 {
            return 0;
        }
        let mut i = i - 1;
        while self.chars[i].is_whitespace() && !self.empty_line(i) && i > 0 {
            i -= 1;
        }
        if !self.chars[i].is_whitespace() {
            let kind = class(self.chars[i]);
            while i > 0 && class(self.chars[i - 1]) == kind {
                i -= 1;
            }
        }
        i
    }

    pub fn input(&mut self, key: KeyEvent) {
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            match key.code {
                KeyCode::Char('a') => self.cursor = self.home(),
                KeyCode::Char('e') => self.cursor = self.end(),
                KeyCode::Char('u') => {
                    let home = self.home();
                    self.chars.drain(home..self.cursor);
                    self.cursor = home;
                }
                KeyCode::Char('w') => {
                    let end = self.cursor;
                    while self.cursor > 0 && self.chars[self.cursor - 1].is_whitespace() {
                        self.cursor -= 1;
                    }
                    while self.cursor > 0 && !self.chars[self.cursor - 1].is_whitespace() {
                        self.cursor -= 1;
                    }
                    self.chars.drain(self.cursor..end);
                }
                KeyCode::Char('j') => self.insert("\n"),
                _ => (),
            }
            return;
        }
        match key.code {
            KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::ALT) => {
                self.insert(&c.to_string())
            }
            KeyCode::Tab => self.insert("    "),
            KeyCode::Enter => self.insert("\n"),
            KeyCode::Left => self.cursor = self.cursor.saturating_sub(1),
            KeyCode::Right => self.cursor = (self.cursor + 1).min(self.chars.len()),
            KeyCode::Home => self.cursor = self.home(),
            KeyCode::End => self.cursor = self.end(),
            KeyCode::Backspace if self.cursor > 0 => {
                self.cursor -= 1;
                self.chars.remove(self.cursor);
            }
            KeyCode::Delete if self.cursor < self.chars.len() => {
                self.chars.remove(self.cursor);
            }
            KeyCode::Up => {
                let home = self.home();
                let col = self.cursor - home;
                if home > 0 {
                    self.cursor = home - 1;
                    self.cursor = (self.home() + col).min(home - 1);
                }
            }
            KeyCode::Down => {
                let col = self.cursor - self.home();
                let end = self.end();
                if end < self.chars.len() {
                    self.cursor = end + 1;
                    self.cursor = (self.cursor + col).min(self.end());
                }
            }
            _ => (),
        }
    }

    // Visual rows and caret use terminal-cell widths, not bytes or code points.
    pub fn layout(&self, width: usize) -> (Vec<String>, usize, usize) {
        let width = width.max(2);
        let mut rows: Vec<String> = Vec::new();
        let (mut caret_x, mut caret_y) = (0, 0);
        let mut offset = 0;
        for line in self.chars.split(|c| *c == '\n') {
            let units: Vec<_> = line
                .iter()
                .map(|c| (c.width().unwrap_or(0), c.is_whitespace()))
                .collect();
            for range in word_ranges(&units, width) {
                // A caret at a soft break belongs to the following visual row.
                if (offset + range.start..=offset + range.end).contains(&self.cursor) {
                    caret_x = units[range.start..self.cursor - offset]
                        .iter()
                        .map(|(cells, _)| cells)
                        .sum::<usize>()
                        .min(width - 1);
                    caret_y = rows.len();
                }
                rows.push(line[range].iter().collect());
            }
            offset += line.len() + 1;
        }
        // Reserve a cell for the end-of-input caret after an exactly full row,
        // without letting that virtual cell influence word wrapping.
        if rows
            .last()
            .unwrap()
            .chars()
            .map(|c| c.width().unwrap_or(0))
            .sum::<usize>()
            >= width
        {
            rows.push(String::new());
            if self.cursor == self.chars.len() {
                caret_x = 0;
                caret_y = rows.len() - 1;
            }
        }
        (rows, caret_x, caret_y)
    }
}

fn class(c: char) -> u8 {
    if c.is_whitespace() {
        0
    } else if c.is_alphanumeric() || c == '_' {
        1
    } else {
        2
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn word_wrap_keeps_caret_on_the_reflowed_text() {
        let mut editor = Editor::new("one two three");
        for (cursor, expected) in [
            (0, (0, 0)),
            (7, (7, 0)),
            (8, (0, 1)),
            (10, (2, 1)),
            (13, (5, 1)),
        ] {
            editor.cursor = cursor;
            let (rows, x, y) = editor.layout(9);
            assert_eq!(rows, ["one two ", "three"]);
            assert_eq!((x, y), expected);
            assert_eq!(editor.text(), "one two three");
        }
        editor.cursor = 8;
        editor.insert("new ");
        assert_eq!(
            editor.layout(9),
            (vec!["one two ".into(), "new three".into(), "".into()], 4, 1)
        );
    }

    #[test]
    fn word_wrap_preserves_newlines_and_full_row_caret() {
        let mut editor = Editor::new("one two\n\n界界 end\n");
        editor.cursor = 13;
        let (rows, x, y) = editor.layout(6);
        assert_eq!(rows, ["one ", "two", "", "界界 ", "end", ""]);
        assert_eq!((x, y), (1, 4));
        assert_eq!(
            Editor::new("abcd").layout(4),
            (vec!["abcd".into(), "".into()], 0, 1)
        );
        assert_eq!(
            Editor::new("abcd\n").layout(4),
            (vec!["abcd".into(), "".into()], 0, 1)
        );
        assert_eq!(Editor::new("").layout(4), (vec!["".into()], 0, 0));
    }

    #[test]
    fn unicode_editing_and_paste() {
        let mut e = Editor::new("a界");
        e.input(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE));
        e.insert("é\r\n\x1b");
        assert_eq!(e.text(), "aé\n界");
        e.input(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE));
        assert_eq!(e.text(), "aé界");
        e.cursor = 3;
        let (lines, x, y) = e.layout(3);
        assert_eq!(lines, vec!["aé", "界"]);
        assert_eq!((x, y), (2, 1));
    }

    fn keys(e: &mut Editor, keys: &str) {
        for c in keys.chars() {
            if c == '\x1b' {
                e.escape();
            } else {
                e.vim(KeyCode::Char(c).into());
            }
        }
    }

    #[test]
    fn vim_motions_rest_on_characters() {
        let mut e = Editor::normal("foo.bar  baz\n\n  qux", 0);
        let mut seen = vec![];
        for _ in 0..6 {
            keys(&mut e, "w");
            seen.push(e.cursor);
        }
        // `.` is its own word; the empty line stops `w`; the end clamps.
        assert_eq!(seen, [3, 4, 9, 13, 16, 18]);
        for expected in [16, 13, 9, 4, 3, 0, 0] {
            keys(&mut e, "b");
            assert_eq!(e.cursor, expected);
        }
        keys(&mut e, "$");
        assert_eq!(e.cursor, 11);
        keys(&mut e, "l");
        assert_eq!(e.cursor, 11);
        keys(&mut e, "0h");
        assert_eq!(e.cursor, 0);
        keys(&mut e, "$j");
        assert_eq!(e.cursor, 13);
        keys(&mut e, "jlll");
        assert_eq!(e.cursor, 17);
        keys(&mut e, "kk");
        assert_eq!(e.cursor, 0);
        keys(&mut e, "lllll");
        // Leaving Insert mode steps back onto the last typed character.
        keys(&mut e, "a");
        assert_eq!((e.mode, e.cursor), (VimMode::Insert, 6));
        keys(&mut e, "\x1b");
        assert_eq!((e.mode, e.cursor), (VimMode::Normal, 5));
    }

    #[test]
    fn vim_edits_and_undo() {
        let mut e = Editor::normal("one\n  two\nthree", 0);
        keys(&mut e, "x");
        assert_eq!(e.text(), "ne\n  two\nthree");
        keys(&mut e, "jdd");
        assert_eq!((e.text().as_str(), e.cursor), ("ne\nthree", 3));
        keys(&mut e, "dd");
        assert_eq!((e.text().as_str(), e.cursor), ("ne", 0));
        // An operator cancelled by Esc or another key does nothing.
        e.vim(KeyCode::Char('d').into());
        assert!(e.escape());
        assert!(!e.escape());
        keys(&mut e, "dxd\x1b");
        assert_eq!(e.text(), "ne");
        keys(&mut e, "oend\x1bOtop\x1b");
        assert_eq!((e.text().as_str(), e.cursor), ("ne\ntop\nend", 5));
        keys(&mut e, "ia\x1b");
        e.vim(KeyCode::Enter.into());
        assert_eq!(e.text(), "ne\ntoap\nend");
        // Each Normal command or Insert session is one undo step;
        // an Insert session without changes is not.
        keys(&mut e, "i\x1bu");
        assert_eq!(e.text(), "ne\ntop\nend");
        for expected in [
            "ne\nend",
            "ne",
            "ne\nthree",
            "ne\n  two\nthree",
            "one\n  two\nthree",
            "one\n  two\nthree",
        ] {
            keys(&mut e, "u");
            assert_eq!(e.text(), expected);
        }
        e.paste("X");
        assert_eq!((e.text().as_str(), e.cursor), ("Xone\n  two\nthree", 1));
        keys(&mut e, "u");
        assert_eq!(e.text(), "one\n  two\nthree");
        keys(&mut e, "dddddddd");
        assert_eq!((e.text().as_str(), e.cursor), ("", 0));
    }

    fn op(text: &str, cursor: usize, cmd: &str) -> (String, usize, VimMode) {
        let mut e = Editor::normal(text, cursor);
        keys(&mut e, cmd);
        (e.text(), e.cursor, e.mode)
    }

    #[test]
    fn vim_operators_take_motions() {
        use VimMode::{Insert, Normal};
        let text = "foo.bar  baz\n\n  qux end";
        for (cursor, cmd, expected) in [
            (0, "dw", (".bar  baz\n\n  qux end", 0, Normal)),
            (3, "dw", ("foobar  baz\n\n  qux end", 3, Normal)),
            (4, "dw", ("foo.baz\n\n  qux end", 4, Normal)),
            // The last word stops at the line end; an empty line is linewise.
            (9, "dw", ("foo.bar  \n\n  qux end", 8, Normal)),
            (13, "dw", ("foo.bar  baz\n  qux end", 15, Normal)),
            (9, "db", ("foo.baz\n\n  qux end", 4, Normal)),
            (5, "d0", ("ar  baz\n\n  qux end", 0, Normal)),
            (5, "d$", ("foo.b\n\n  qux end", 4, Normal)),
            (5, "dh", ("foo.ar  baz\n\n  qux end", 4, Normal)),
            (5, "dl", ("foo.br  baz\n\n  qux end", 5, Normal)),
            (0, "dj", ("  qux end", 2, Normal)),
            (16, "dk", ("foo.bar  baz", 0, Normal)),
            (16, "dd", ("foo.bar  baz\n", 13, Normal)),
            // `cw` keeps the following blank; on blanks it acts like `dw`.
            (4, "cw", ("foo.  baz\n\n  qux end", 4, Insert)),
            (7, "cw", ("foo.barbaz\n\n  qux end", 7, Insert)),
            (13, "cw", ("foo.bar  baz\n\n  qux end", 13, Insert)),
            (9, "cb", ("foo.baz\n\n  qux end", 4, Insert)),
            (5, "c0", ("ar  baz\n\n  qux end", 0, Insert)),
            (5, "c$", ("foo.b\n\n  qux end", 5, Insert)),
            (5, "ch", ("foo.ar  baz\n\n  qux end", 4, Insert)),
            (5, "cl", ("foo.br  baz\n\n  qux end", 5, Insert)),
            (0, "cj", ("\n  qux end", 0, Insert)),
            (16, "ck", ("foo.bar  baz\n", 13, Insert)),
            (16, "cc", ("foo.bar  baz\n\n", 14, Insert)),
            // Failed motions and other keys cancel the operator.
            (0, "dh", (text, 0, Normal)),
            (0, "cb", (text, 0, Normal)),
            (0, "ck", (text, 0, Normal)),
            (16, "dj", (text, 16, Normal)),
            (0, "dxl", (text, 1, Normal)),
        ] {
            let expected = (expected.0.to_string(), expected.1, expected.2);
            assert_eq!(op(text, cursor, cmd), expected, "{cmd} at {cursor}");
        }
        // `db` from column 0 ends at the previous line's end, linewise from
        // leading blanks.
        assert_eq!(op("a bc\nd", 5, "db"), ("a \nd".into(), 1, Normal));
        assert_eq!(op("  bc\nd", 5, "db"), ("d".into(), 0, Normal));
    }

    #[test]
    fn vim_change_is_one_undo_step() {
        let mut e = Editor::normal("one two", 0);
        keys(&mut e, "cwthree\x1b");
        assert_eq!(e.text(), "three two");
        keys(&mut e, "u");
        assert_eq!(e.text(), "one two");
        // A no-op delete is not an undo step.
        keys(&mut e, "xd0u");
        assert_eq!(e.text(), "one two");
    }
}
