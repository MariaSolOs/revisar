use crate::diff::{FileDiff, Kind, display_text};
use ratatui::{
    style::{Color, Modifier, Style},
    text::Span,
};
use syntect::{
    easy::HighlightLines,
    highlighting::{FontStyle, ThemeSet},
    parsing::SyntaxSet,
};

// Fixed palette from ~/.config/tuicr/themes/miss-dracula.toml.
pub const BG: Color = Color::Rgb(0x0e, 0x14, 0x19);
pub const FG: Color = Color::Rgb(0xf6, 0xf6, 0xf5);
pub const MUTED: Color = Color::Rgb(0xa9, 0xab, 0xac);
pub const DIM: Color = Color::Rgb(0x6d, 0x59, 0x78);
pub const SELECTION: Color = Color::Rgb(0x3c, 0x41, 0x48);
pub const ADD: Color = Color::Rgb(0x97, 0xed, 0xa2);
pub const ADD_BG: Color = Color::Rgb(0x22, 0x37, 0x2c);
pub const DEL: Color = Color::Rgb(0xec, 0x6a, 0x88);
pub const DEL_BG: Color = Color::Rgb(0x34, 0x22, 0x31);
pub const CONTEXT: Color = Color::Rgb(0xb0, 0x8b, 0xbb);
pub const PURPLE: Color = Color::Rgb(0xba, 0xa0, 0xe8);
pub const PINK: Color = Color::Rgb(0xe4, 0x8c, 0xc1);
pub const BORDER: Color = Color::Rgb(0x62, 0x72, 0xa4);
pub const BAR: Color = Color::Rgb(0x1e, 0x1f, 0x29);
pub const CURSOR: Color = Color::Rgb(0xe1, 0x12, 0x99);
pub const CYAN: Color = Color::Rgb(0xa7, 0xdf, 0xef);
pub const YELLOW: Color = Color::Rgb(0xe8, 0xed, 0xa2);
pub const GREEN: Color = Color::Rgb(0x87, 0xe5, 0x8e);
pub const RED: Color = Color::Rgb(0xe9, 0x56, 0x78);

pub fn base() -> Style {
    Style::default().bg(BG).fg(FG)
}

pub struct Syntax {
    syntaxes: SyntaxSet,
    theme: syntect::highlighting::Theme,
}

impl Default for Syntax {
    fn default() -> Self {
        Self {
            syntaxes: two_face::syntax::extra_newlines(),
            theme: ThemeSet::load_from_reader(&mut std::io::Cursor::new(include_bytes!(
                "../assets/miss-dracula-syntax.tmTheme"
            )))
            .expect("embedded syntax theme must be valid"),
        }
    }
}

impl Syntax {
    pub fn highlight(&self, file: &FileDiff) -> Vec<Vec<Span<'static>>> {
        let syntax = std::path::Path::new(&file.path)
            .extension()
            .and_then(|s| s.to_str())
            .and_then(|ext| self.syntaxes.find_syntax_by_extension(ext))
            .or_else(|| self.syntaxes.find_syntax_by_name(&file.path))
            .unwrap_or_else(|| self.syntaxes.find_syntax_plain_text());
        let mut old = HighlightLines::new(syntax, &self.theme);
        let mut new = HighlightLines::new(syntax, &self.theme);
        file.rows
            .iter()
            .map(|row| {
                if row.kind == Kind::Hunk {
                    // Omitted source between hunks is unknown; don't carry stale parser state.
                    old = HighlightLines::new(syntax, &self.theme);
                    new = HighlightLines::new(syntax, &self.theme);
                    return vec![Span::styled(
                        display_text(&row.text),
                        Style::default().fg(PURPLE),
                    )];
                }
                if row.kind == Kind::Meta {
                    return vec![Span::styled(
                        display_text(&row.text),
                        Style::default().fg(MUTED),
                    )];
                }
                let text = format!("{}\n", row.text);
                if row.kind == Kind::Context {
                    let _ = old.highlight_line(&text, &self.syntaxes);
                }
                let highlighter = if row.kind == Kind::Delete {
                    &mut old
                } else {
                    &mut new
                };
                let Ok(highlighted) = highlighter.highlight_line(&text, &self.syntaxes) else {
                    // Syntax errors must never make reviewed code disappear.
                    return vec![Span::styled(
                        display_text(&row.text),
                        Style::default().fg(FG),
                    )];
                };
                highlighted
                    .into_iter()
                    .filter_map(|(s, text)| {
                        let text = text.trim_end_matches('\n');
                        if text.is_empty() {
                            return None;
                        }
                        let c = s.foreground;
                        let mut style = Style::default().fg(Color::Rgb(c.r, c.g, c.b));
                        if s.font_style.contains(FontStyle::BOLD) {
                            style = style.add_modifier(Modifier::BOLD);
                        }
                        if s.font_style.contains(FontStyle::ITALIC) {
                            style = style.add_modifier(Modifier::ITALIC);
                        }
                        if s.font_style.contains(FontStyle::UNDERLINE) {
                            style = style.add_modifier(Modifier::UNDERLINED);
                        }
                        Some(Span::styled(display_text(text), style))
                    })
                    .collect()
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn embedded_theme_highlights_rust_and_typescript() {
        let syntax = Syntax::default();
        for path in ["main.rs", "main.ts"] {
            let f = FileDiff {
                path: path.into(),
                status: 'A',
                patch: vec![],
                rows: crate::diff::parse_patch("@@ -0,0 +1 @@\n+const value = \"hello\";\n")
                    .unwrap(),
            };
            let colors = syntax.highlight(&f);
            assert!(colors[1].iter().any(|s| s.style.fg == Some(PINK)));
            assert!(colors[1].iter().any(|s| s.style.fg == Some(YELLOW)));
        }
    }
}
