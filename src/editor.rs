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
        let mut rows = vec![String::new()];
        let (mut x, mut caret_x, mut caret_y) = (0, 0, 0);
        for (i, c) in self.chars.iter().chain(std::iter::once(&' ')).enumerate() {
            let w = c.width().unwrap_or(0);
            if *c != '\n' && x + w > width {
                rows.push(String::new());
                x = 0;
            }
            if i == self.cursor {
                caret_x = x.min(width - 1);
                caret_y = rows.len() - 1;
            }
            if i == self.chars.len() {
                break;
            }
            if *c == '\n' {
                rows.push(String::new());
                x = 0;
            } else {
                rows.last_mut().unwrap().push(*c);
                x += w;
            }
        }
        (rows, caret_x, caret_y)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
