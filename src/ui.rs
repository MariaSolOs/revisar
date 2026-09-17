use crate::{
    app::{App, Confirmation, Mode},
    diff::{Kind, display_text},
    editor::Editor,
    theme::{self as t, Syntax},
};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap},
};
use std::collections::HashMap;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

#[derive(Default)]
pub struct Ui {
    syntax: Syntax,
    highlighted: HashMap<usize, Vec<Vec<Span<'static>>>>,
    files: ListState,
    comments: ListState,
}

fn block(title: impl Into<Line<'static>>, focused: bool) -> Block<'static> {
    Block::default()
        .borders(Borders::ALL)
        .title(title)
        .border_style(Style::default().fg(if focused { t::PINK } else { t::BORDER }))
        .style(t::base())
}

impl Ui {
    pub fn draw(&mut self, frame: &mut Frame, app: &mut App) {
        let area = frame.area();
        frame.render_widget(Block::default().style(t::base()), area);
        if area.width < 45 || area.height < 12 {
            frame.render_widget(
                Paragraph::new(
                    "revisar: resize to at least 45 columns x 12 rows.\nq cancels; ? shows help.",
                )
                .wrap(Wrap { trim: false }),
                area,
            );
            return;
        }
        let layout = Layout::vertical([
            Constraint::Length(1),
            Constraint::Min(1),
            Constraint::Length(2),
        ])
        .split(area);
        let reviewed = app.reviewed.iter().filter(|r| **r).count();
        let header = format!(
            " revisar  |  working tree  |  {reviewed}/{} reviewed  |  {} comments",
            app.snapshot.files.len(),
            app.comments.len()
        );
        frame.render_widget(
            Paragraph::new(header).style(Style::default().fg(t::PURPLE).bg(t::BAR)),
            layout[0],
        );
        let sidebar = if area.width >= 85 || app.files_focused {
            (area.width / 4).clamp(22, 38)
        } else {
            0
        };
        let body =
            Layout::horizontal([Constraint::Length(sidebar), Constraint::Min(1)]).split(layout[1]);
        if sidebar > 0 {
            self.files(frame, app, body[0]);
        }
        if app.summary {
            self.summary(frame, app, body[1]);
        } else {
            self.diff(frame, app, body[1]);
        }
        let hints = if app.summary {
            " s/Esc diff  j/k comments  Enter jump  i edit  d delete  Ctrl-d/u scroll  S Send"
        } else if app.selection.is_some() {
            " RANGE  j/k extend  c comment  Esc cancel"
        } else {
            " j/k move  Tab files  c comment  v range  s summary  S Send  q cancel  ? help"
        };
        frame.render_widget(
            Paragraph::new(vec![
                Line::styled(hints, Style::default().fg(t::CONTEXT)),
                Line::styled(display_text(&app.message), Style::default().fg(t::CYAN)),
            ])
            .style(Style::default().bg(t::BAR)),
            layout[2],
        );
        match &mut app.mode {
            Mode::Comment(draft) => {
                let title = format!(" Comment: {} ", draft.anchor.heading());
                editor_popup(
                    frame,
                    &title,
                    &draft.editor,
                    "Enter/Ctrl-s: keep comment   Shift-Enter/Ctrl-j: newline   Esc: discard edit",
                    75,
                    65,
                );
            }
            Mode::Search(editor) => editor_popup(
                frame,
                " Search all diffs (case-insensitive) ",
                editor,
                "Enter: search   Esc: cancel",
                75,
                30,
            ),
            Mode::Confirm(c) => {
                let prompt = match c {
                    Confirmation::Send => format!("Send {} comment(s) to the agent and close?\n\n{reviewed}/{} files marked reviewed. Nothing is saved for later.", app.comments.len(), app.snapshot.files.len()),
                    Confirmation::Stale => "The working tree changed since this review opened.\n\nSend comments with an explicit stale-snapshot warning? The agent will receive the original code excerpts and line anchors.".into(),
                    Confirmation::Discard => format!("Discard all {} comment(s) and close?\n\nNothing will be sent or saved.", app.comments.len()),
                    Confirmation::Delete(_) => "Delete this comment from the review?".into(),
                };
                popup(
                    frame,
                    " Confirm ",
                    &format!("{prompt}\n\ny/Enter: yes     n/Esc: no"),
                    70,
                    55,
                );
            }
            Mode::Help(scroll) => {
                let (area, lines) = help_layout(frame.area());
                frame.render_widget(Clear, area);
                let b = block(HELP_TITLE, true);
                let inner = b.inner(area);
                frame.render_widget(b, area);
                *scroll = (*scroll).min(lines.len().saturating_sub(inner.height as usize));
                let visible: Vec<Line> = lines
                    .into_iter()
                    .skip(*scroll)
                    .take(inner.height as usize)
                    .map(Line::raw)
                    .collect();
                frame.render_widget(Paragraph::new(visible), inner);
            }
            Mode::Normal => (),
        }
    }

    fn files(&mut self, frame: &mut Frame, app: &App, area: Rect) {
        let items: Vec<_> = app
            .snapshot
            .files
            .iter()
            .enumerate()
            .map(|(i, f)| {
                let color = match f.status {
                    'A' => t::GREEN,
                    'D' => t::RED,
                    _ => t::YELLOW,
                };
                let comments = app
                    .comments
                    .iter()
                    .filter(|c| c.anchor.path.as_deref() == Some(&f.path))
                    .count();
                ListItem::new(Line::from(vec![
                    Span::styled(
                        if app.reviewed[i] { "[x] " } else { "[ ] " },
                        Style::default().fg(if app.reviewed[i] { t::ADD } else { t::DIM }),
                    ),
                    Span::styled(format!("{} ", f.status), Style::default().fg(color)),
                    Span::raw(display_text(&f.path)),
                    Span::styled(
                        if comments > 0 {
                            format!(" ({comments})")
                        } else {
                            String::new()
                        },
                        Style::default().fg(t::CYAN),
                    ),
                ]))
            })
            .collect();
        self.files.select((!items.is_empty()).then_some(app.file));
        frame.render_stateful_widget(
            List::new(items)
                .block(block(" Files ", app.files_focused))
                .highlight_style(Style::default().bg(t::SELECTION))
                .highlight_symbol(">"),
            area,
            &mut self.files,
        );
    }

    fn diff(&mut self, frame: &mut Frame, app: &mut App, area: Rect) {
        let title = app
            .snapshot
            .files
            .get(app.file)
            .map(|f| format!(" {} ", display_text(&f.path)))
            .unwrap_or(" Working tree ".into());
        let b = block(title, !app.files_focused);
        let inner = b.inner(area);
        frame.render_widget(b, area);
        app.height = inner.height as usize;
        let Some(file) = app.snapshot.files.get(app.file) else {
            frame.render_widget(
                Paragraph::new("No working-tree changes.\n\na: general comment   q: close")
                    .style(Style::default().fg(t::MUTED)),
                inner,
            );
            return;
        };
        let highlighted = self
            .highlighted
            .entry(app.file)
            .or_insert_with(|| self.syntax.highlight(file));
        let view = &mut app.views[app.file];
        if view.row < view.top {
            view.top = view.row;
        }
        if view.row >= view.top + inner.height as usize {
            view.top = view.row + 1 - inner.height as usize;
        }
        let view = *view;
        for (screen_row, row_index) in (view.top..file.rows.len())
            .take(inner.height as usize)
            .enumerate()
        {
            let row = &file.rows[row_index];
            let selected = app
                .selection
                .is_some_and(|a| (a.min(view.row)..=a.max(view.row)).contains(&row_index));
            let current = row_index == view.row;
            let bg = if current || selected {
                t::SELECTION
            } else {
                match row.kind {
                    Kind::Add => t::ADD_BG,
                    Kind::Delete => t::DEL_BG,
                    _ => t::BG,
                }
            };
            let fg = match row.kind {
                Kind::Add => t::ADD,
                Kind::Delete => t::DEL,
                Kind::Hunk => t::PURPLE,
                _ => t::CONTEXT,
            };
            let has_comment = app
                .comments
                .iter()
                .any(|c| app.comment_on_row(c, row_index));
            let marker = if current {
                ">"
            } else if selected {
                "|"
            } else {
                " "
            };
            let old = row.old.map_or(String::new(), |n| n.to_string());
            let new = row.new.map_or(String::new(), |n| n.to_string());
            let prefix = match row.kind {
                Kind::Add => "+",
                Kind::Delete => "-",
                _ => " ",
            };
            let mut spans = vec![
                Span::styled(marker, Style::default().fg(t::CURSOR)),
                Span::styled(
                    if has_comment { "*" } else { " " },
                    Style::default().fg(t::CYAN),
                ),
                Span::styled(
                    format!("{old:>5} {new:>5} {prefix} "),
                    Style::default().fg(fg),
                ),
            ];
            let gutter: usize = spans.iter().map(Span::width).sum();
            let width = (inner.width as usize).saturating_sub(gutter);
            let (mut code, more) =
                crop(&highlighted[row_index], view.left, width.saturating_sub(1));
            if app.matches.binary_search(&(app.file, row_index)).is_ok() {
                for s in &mut code {
                    s.style = s.style.add_modifier(Modifier::UNDERLINED);
                }
            }
            spans.extend(code);
            if more {
                spans.push(Span::styled(">", Style::default().fg(t::PURPLE)));
            }
            let rect = Rect::new(inner.x, inner.y + screen_row as u16, inner.width, 1);
            frame.render_widget(
                Paragraph::new(Line::from(spans)).style(Style::default().bg(bg).fg(fg)),
                rect,
            );
        }
    }

    fn summary(&mut self, frame: &mut Frame, app: &mut App, area: Rect) {
        let b = block(
            " Comments - Enter jump / i edit / d delete ",
            !app.files_focused,
        );
        let inner = b.inner(area);
        frame.render_widget(b, area);
        if app.comments.is_empty() {
            frame.render_widget(
                Paragraph::new("No comments yet. s returns to the diff; a adds a general comment."),
                inner,
            );
            return;
        }
        let columns = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length((inner.height / 3).clamp(2, 8)),
                Constraint::Min(1),
            ])
            .split(inner);
        let items: Vec<_> = app
            .comments
            .iter()
            .enumerate()
            .map(|(i, c)| ListItem::new(format!("{}. {}", i + 1, c.anchor.heading())))
            .collect();
        self.comments.select(Some(app.comment));
        frame.render_stateful_widget(
            List::new(items)
                .highlight_symbol("> ")
                .highlight_style(Style::default().bg(t::SELECTION).fg(t::CYAN)),
            columns[0],
            &mut self.comments,
        );
        let comment = &app.comments[app.comment];
        let text = format!(
            "{}\n\n{}{}",
            comment.anchor.heading(),
            comment.body,
            if comment.anchor.excerpt.is_empty() {
                String::new()
            } else {
                format!("\n\nReviewed code:\n{}", comment.anchor.excerpt)
            }
        );
        let text = text
            .split('\n')
            .map(display_text)
            .collect::<Vec<_>>()
            .join("\n");
        let editor = Editor::new(&text);
        let (lines, _, _) = editor.layout(columns[1].width as usize);
        app.height = columns[1].height as usize;
        app.summary_scroll = app
            .summary_scroll
            .min(lines.len().saturating_sub(app.height));
        let visible: Vec<Line> = lines
            .iter()
            .skip(app.summary_scroll)
            .take(app.height)
            .map(|s| Line::raw(display_text(s)))
            .collect();
        frame.render_widget(Paragraph::new(visible), columns[1]);
    }
}

fn crop(spans: &[Span<'static>], left: usize, width: usize) -> (Vec<Span<'static>>, bool) {
    let mut x = 0;
    let mut result = Vec::new();
    let mut more = false;
    for span in spans {
        let mut text = String::new();
        for c in span.content.chars() {
            let w = c.width().unwrap_or(0);
            if x >= left && x + w <= left + width {
                text.push(c);
            } else if x < left && x + w > left {
                text.push(' ');
            } else if x + w > left + width {
                more = true;
            }
            x += w;
        }
        if !text.is_empty() {
            result.push(Span::styled(text, span.style));
        }
    }
    (result, more)
}

fn centered(area: Rect, width: u16, height: u16) -> Rect {
    let w = (u32::from(area.width) * u32::from(width) / 100) as u16;
    let h = (u32::from(area.height) * u32::from(height) / 100) as u16;
    Rect::new(
        area.x + (area.width - w) / 2,
        area.y + (area.height - h) / 2,
        w,
        h,
    )
}

fn help_layout(area: Rect) -> (Rect, Vec<String>) {
    // Fit the text plus its border, leaving a one-cell terminal margin.
    // Measure height after wrapping so narrow terminals can still scroll.
    let content_width = HELP.lines().map(UnicodeWidthStr::width).max().unwrap_or(0);
    let width = (content_width.max(HELP_TITLE.width()) + 2)
        .min(area.width.saturating_sub(2) as usize) as u16;
    let (lines, _, _) = Editor::new(HELP).layout(width.saturating_sub(2) as usize);
    let height = (lines.len() + 2).min(area.height.saturating_sub(2) as usize) as u16;
    let rect = Rect::new(
        area.x + (area.width - width) / 2,
        area.y + (area.height - height) / 2,
        width,
        height,
    );
    (rect, lines)
}

fn popup(frame: &mut Frame, title: &str, text: &str, width: u16, height: u16) {
    let area = centered(frame.area(), width, height);
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(text)
            .wrap(Wrap { trim: false })
            .block(block(title.to_string(), true)),
        area,
    );
}

fn editor_popup(
    frame: &mut Frame,
    title: &str,
    editor: &Editor,
    hint: &str,
    width: u16,
    height: u16,
) {
    let area = centered(frame.area(), width, height);
    frame.render_widget(Clear, area);
    let b = block(title.to_string(), true);
    let inner = b.inner(area);
    frame.render_widget(b, area);
    let sections = Layout::vertical([Constraint::Min(1), Constraint::Length(2)]).split(inner);
    let (lines, x, y) = editor.layout(sections[0].width as usize);
    let top = y.saturating_sub(sections[0].height.saturating_sub(1) as usize);
    let visible: Vec<Line> = lines
        .iter()
        .skip(top)
        .take(sections[0].height as usize)
        .map(|s| Line::raw(s.clone()))
        .collect();
    frame.render_widget(Paragraph::new(visible), sections[0]);
    frame.render_widget(
        Paragraph::new(hint)
            .style(Style::default().fg(t::CONTEXT))
            .wrap(Wrap { trim: false }),
        sections[1],
    );
    frame.set_cursor_position((sections[0].x + x as u16, sections[0].y + (y - top) as u16));
}

const HELP_TITLE: &str = " Help - j/k scroll, ?/Esc close ";
const HELP: &str = "NAVIGATE\n j/k or arrows    Move through lines / files / comments\n h/l               Horizontal diff scroll\n Ctrl-d/u          Half-page down/up (summary: scroll body)\n g/G               First/last row\n Tab               Focus files or diff\n {/}               Previous/next file\n [/]               Previous/next hunk in this file\n / then n/N        Search all diffs, next/previous match\n\nCOMMENT\n c                 Line comment (metadata: file comment)\n v then j/k, c     Range comment (one side, one hunk)\n C / a             File / general comment\n s                 Comment summary; Enter jumps to code\n i / d             Edit / delete selected comment\n Enter or Ctrl-s   Keep comment in memory\n Shift-Enter / Ctrl-j   Newline while editing\n Esc               Cancel edit / selection / search\n\nFINISH\n r                 Toggle file reviewed (in memory only)\n S                 Send all comments and close\n q                 Cancel; confirm discarding comments\n\n? / Esc / q closes help.";

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diff::{FileDiff, Snapshot, parse_patch};
    use ratatui::{Terminal, backend::TestBackend};
    #[test]
    fn help_fits_its_contents_instead_of_filling_large_terminals() {
        let expected_width = HELP.lines().map(UnicodeWidthStr::width).max().unwrap() as u16 + 2;
        let expected_height = HELP.lines().count() as u16 + 2;
        for area in [Rect::new(0, 0, 120, 40), Rect::new(5, 3, 200, 80)] {
            let (popup, lines) = help_layout(area);
            assert_eq!(popup.width, expected_width);
            assert_eq!(popup.height, expected_height);
            assert_eq!(popup.x, area.x + (area.width - popup.width) / 2);
            assert_eq!(popup.y, area.y + (area.height - popup.height) / 2);
            assert_eq!(lines.join("\n"), HELP);
        }
    }

    #[test]
    fn help_wraps_and_scrolls_on_small_terminals_then_resets_on_resize() {
        let mut app = App::new(Snapshot {
            root: "/repo".into(),
            head: "abc".into(),
            files: vec![],
        });
        let mut ui = Ui::default();
        app.mode = Mode::Help(usize::MAX);
        let area = Rect::new(0, 0, 45, 12);
        let (popup, lines) = help_layout(area);
        assert_eq!((popup.width, popup.height), (43, 10));
        assert!(
            lines
                .iter()
                .all(|line| line.width() <= popup.width as usize - 2)
        );
        assert!(lines.len() > popup.height as usize - 2);
        let mut terminal = Terminal::new(TestBackend::new(area.width, area.height)).unwrap();
        terminal.draw(|f| ui.draw(f, &mut app)).unwrap();
        assert!(matches!(app.mode, Mode::Help(scroll) if scroll == lines.len() - 8));

        let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
        terminal.draw(|f| ui.draw(f, &mut app)).unwrap();
        assert!(matches!(app.mode, Mode::Help(0)));
    }

    #[test]
    fn render_diff_summary_editor_and_small_terminal() {
        let mut a = App::new(Snapshot {
            root: "/repo".into(),
            head: "abc".into(),
            files: vec![FileDiff {
                path: "src/main.rs".into(),
                status: 'A',
                patch: vec![],
                rows: parse_patch("@@ -0,0 +1 @@\n+const X: &str = \"界\";\n").unwrap(),
            }],
        });
        let mut ui = Ui::default();
        for (w, h) in [(120, 40), (60, 20), (20, 5)] {
            let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
            terminal.draw(|f| ui.draw(f, &mut a)).unwrap();
            a.summary = true;
            terminal.draw(|f| ui.draw(f, &mut a)).unwrap();
            a.summary = false;
            a.mode = Mode::Search(Editor::new("hello界"));
            terminal.draw(|f| ui.draw(f, &mut a)).unwrap();
            a.mode = Mode::Normal;
        }
    }
}
