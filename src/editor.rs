use crate::wrap::word_ranges;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use unicode_width::UnicodeWidthChar;

#[derive(Default)]
pub struct Editor {
    pub chars: Vec<char>,
    pub cursor: usize,
}

impl Editor {
    pub fn new(s: &str) -> Self {
        let chars: Vec<_> = s.chars().collect();
        Self {
            cursor: chars.len(),
            chars,
        }
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

    fn home(&self) -> usize {
        self.chars[..self.cursor]
            .iter()
            .rposition(|c| *c == '\n')
            .map_or(0, |i| i + 1)
    }

    fn end(&self) -> usize {
        self.chars[self.cursor..]
            .iter()
            .position(|c| *c == '\n')
            .map_or(self.chars.len(), |i| self.cursor + i)
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
}
