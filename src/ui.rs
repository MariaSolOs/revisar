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
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Padding, Paragraph, Wrap},
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
        .title_style(if focused {
            Style::default().add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        })
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
        let header = format!(
            " revisar  |  working tree  |  {} files  |  {} comments",
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
                let area = comment_layout(frame.area(), &draft.editor);
                editor_popup(frame, area, &title, &draft.editor, COMMENT_HINT);
            }
            Mode::Search(editor) => editor_popup(
                frame,
                centered(frame.area(), 75, 30),
                " Search all diffs (case-insensitive) ",
                editor,
                "Enter: search   Esc: cancel",
            ),
            Mode::Confirm(c) => {
                let prompt = match c {
                    Confirmation::Send => format!("Send {} comment(s) to the agent and close?\n\nNothing is saved for later.", app.comments.len()),
                    Confirmation::Stale => "The working tree changed since this review opened.\n\nSend comments with an explicit stale-snapshot warning? The agent will receive the original code excerpts and line anchors.".into(),
                    Confirmation::Discard => format!("Discard all {} comment(s) and close?\n\nNothing will be sent or saved.", app.comments.len()),
                    Confirmation::Delete(_) => "Delete this comment from the review?".into(),
                };
                let text = format!("{prompt}\n\ny/Enter: yes     n/Esc: no");
                let (area, paragraph) = confirmation_layout(frame.area(), " Confirm ", &text);
                frame.render_widget(Clear, area);
                frame.render_widget(paragraph, area);
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
            .map(|f| {
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
        if std::mem::take(&mut view.center) {
            // Like zz: allow blank space below EOF, but never scroll above BOF.
            view.top = view.row.saturating_sub(inner.height as usize / 2);
        }
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
            let has_comment = app
                .comments
                .iter()
                .any(|c| app.comment_on_row(c, row_index));
            let bg = if has_comment && (current || selected) {
                t::COMMENT_SELECTED_BG
            } else if current || selected {
                t::SELECTION
            } else if has_comment {
                t::COMMENT_BG
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

fn confirmation_layout<'a>(area: Rect, title: &str, text: &'a str) -> (Rect, Paragraph<'a>) {
    let content_width = text.lines().map(UnicodeWidthStr::width).max().unwrap_or(0);
    // One cell of horizontal padding plus the border on each side. Cap long
    // prompts at a readable width rather than stretching across the terminal.
    let width = (content_width.max(title.width()) + 4)
        .min(64)
        .min(area.width.saturating_sub(2) as usize) as u16;
    let paragraph = Paragraph::new(text).wrap(Wrap { trim: false });
    // Measure with the same word wrapper used for rendering. Allow the dialog
    // to use the terminal's full height if needed to keep the choices visible.
    let height =
        (paragraph.line_count(width.saturating_sub(4)) + 2).min(area.height as usize) as u16;
    let rect = Rect::new(
        area.x + (area.width - width) / 2,
        area.y + (area.height - height) / 2,
        width,
        height,
    );
    let paragraph = paragraph.block(block(title.to_string(), true).padding(Padding::horizontal(1)));
    (rect, paragraph)
}

fn comment_layout(area: Rect, editor: &Editor) -> Rect {
    // Keep the width steady while typing; grow only as wrapped content needs
    // more rows. Longer comments scroll within a bounded editing area.
    let width = 64.min(area.width.saturating_sub(2));
    let inner_width = width.saturating_sub(2);
    let (lines, _, _) = editor.layout(inner_width as usize);
    let hint_height = Paragraph::new(COMMENT_HINT)
        .wrap(Wrap { trim: false })
        .line_count(inner_width);
    let height = (lines.len().clamp(3, 10) + hint_height + 2)
        .min(area.height.saturating_sub(2) as usize) as u16;
    Rect::new(
        area.x + (area.width - width) / 2,
        area.y + (area.height - height) / 2,
        width,
        height,
    )
}

fn editor_popup(frame: &mut Frame, area: Rect, title: &str, editor: &Editor, hint: &str) {
    frame.render_widget(Clear, area);
    let b = block(title.to_string(), true);
    let inner = b.inner(area);
    frame.render_widget(b, area);
    let hint = Paragraph::new(hint)
        .style(Style::default().fg(t::CONTEXT))
        .wrap(Wrap { trim: false });
    let hint_height = hint
        .line_count(inner.width)
        .min(inner.height.saturating_sub(1) as usize) as u16;
    let sections =
        Layout::vertical([Constraint::Min(1), Constraint::Length(hint_height)]).split(inner);
    let (lines, x, y) = editor.layout(sections[0].width as usize);
    let top = y.saturating_sub(sections[0].height.saturating_sub(1) as usize);
    let visible: Vec<Line> = lines
        .iter()
        .skip(top)
        .take(sections[0].height as usize)
        .map(|s| Line::raw(s.clone()))
        .collect();
    frame.render_widget(Paragraph::new(visible), sections[0]);
    frame.render_widget(hint, sections[1]);
    frame.set_cursor_position((sections[0].x + x as u16, sections[0].y + (y - top) as u16));
}

const COMMENT_HINT: &str = "Enter/Ctrl-s: keep   Esc: discard\nShift-Enter/Ctrl-j: newline";
const HELP_TITLE: &str = " Help - j/k scroll, ?/Esc close ";
const HELP: &str = "NAVIGATE\n j/k or arrows    Move through lines / files / comments\n h/l               Horizontal diff scroll\n Ctrl-d/u          Half-page + center (summary: scroll body)\n g/G               First/last row\n Tab               Focus files or diff\n {/}               Previous/next file\n [/]               Center previous/next hunk in this file\n / then n/N        Search all diffs; center next/prev match\n\nCOMMENT\n c                 Line comment (metadata: file comment)\n v then j/k, c     Range comment (one side, one hunk)\n C / a             File / general comment\n s                 Comment summary; Enter jumps to code\n i / d             Edit / delete selected comment\n Enter or Ctrl-s   Keep comment in memory\n Shift-Enter / Ctrl-j   Newline while editing\n Esc               Cancel edit / selection / search\n\nFINISH\n S                 Send all comments and close\n q                 Cancel; confirm discarding comments\n\n? / Esc / q closes help.";

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diff::{FileDiff, Snapshot, parse_patch};
    use ratatui::{Terminal, backend::TestBackend};
    #[test]
    fn commented_lines_and_ranges_have_full_width_backgrounds() {
        use crate::review::{Anchor, Comment};
        let file = FileDiff {
            path: "file.txt".into(),
            status: 'M',
            patch: vec![],
            rows: parse_patch(
                "@@ -10,4 +10,4 @@\n context\n-old one\n-old two\n+new one\n+new two\n tail\n",
            )
            .unwrap(),
        };
        let comment = |a, b| Comment {
            anchor: Anchor::lines(&file, a, b).unwrap(),
            body: "Review this".into(),
        };
        let old = comment(2, 3);
        let new = comment(4, 5);
        let context = comment(1, 1);
        let file_comment = Comment {
            anchor: Anchor::file(&file),
            body: "File comment".into(),
        };
        let mut app = App::new(Snapshot {
            root: "/repo".into(),
            head: "abc".into(),
            files: vec![file],
        });
        let mut ui = Ui::default();
        let mut terminal = Terminal::new(TestBackend::new(80, 14)).unwrap();
        let area = Rect::new(0, 0, 80, 14);
        let inner = block("", true).inner(area);
        let assert_row = |terminal: &Terminal<TestBackend>, row: u16, bg| {
            for x in inner.x..inner.right() {
                assert_eq!(
                    terminal.backend().buffer()[(x, inner.y + row)].bg,
                    bg,
                    "Wrong background at column {x}, diff row {row}"
                );
            }
        };

        app.comments.push(old);
        terminal
            .draw(|frame| ui.diff(frame, &mut app, area))
            .unwrap();
        assert_row(&terminal, 2, t::COMMENT_BG);
        assert_row(&terminal, 3, t::COMMENT_BG);
        // New-side lines with the same source numbers must not inherit it.
        assert_row(&terminal, 4, t::ADD_BG);
        assert_row(&terminal, 5, t::ADD_BG);
        assert_row(&terminal, 1, t::BG);

        app.comments.extend([new, context]);
        terminal
            .draw(|frame| ui.diff(frame, &mut app, area))
            .unwrap();
        for row in 1..=5 {
            assert_row(&terminal, row, t::COMMENT_BG);
        }
        assert_row(&terminal, 6, t::BG);
        let buffer = terminal.backend().buffer();
        assert_eq!(buffer[(inner.x + 1, inner.y + 2)].symbol(), "*");
        // Keep the +/- gutters and syntax foregrounds intact.
        assert_eq!(buffer[(inner.x + 14, inner.y + 2)].fg, t::DEL);
        assert_eq!(buffer[(inner.x + 14, inner.y + 4)].fg, t::ADD);
        assert_eq!(buffer[(inner.x + 16, inner.y + 4)].fg, t::FG);

        app.views[0].row = 4;
        app.selection = Some(2);
        terminal
            .draw(|frame| ui.diff(frame, &mut app, area))
            .unwrap();
        for row in 2..=4 {
            assert_row(&terminal, row, t::COMMENT_SELECTED_BG);
        }
        assert_row(&terminal, 5, t::COMMENT_BG);
        assert_eq!(
            terminal.backend().buffer()[(inner.x, inner.y + 4)].symbol(),
            ">"
        );

        app.selection = None;
        app.views[0].row = 6;
        app.comments.clear();
        terminal
            .draw(|frame| ui.diff(frame, &mut app, area))
            .unwrap();
        for row in [2, 3] {
            assert_row(&terminal, row, t::DEL_BG);
        }
        for row in [4, 5] {
            assert_row(&terminal, row, t::ADD_BG);
        }
        assert_row(&terminal, 1, t::BG);
        assert_row(&terminal, 6, t::SELECTION);

        app.comments.push(file_comment);
        terminal
            .draw(|frame| ui.diff(frame, &mut app, area))
            .unwrap();
        assert_row(&terminal, 0, t::COMMENT_BG);
        assert_row(&terminal, 1, t::BG);
    }

    #[test]
    fn only_focused_panel_titles_are_bold() {
        for focused in [false, true] {
            let mut terminal = Terminal::new(TestBackend::new(20, 3)).unwrap();
            terminal
                .draw(|frame| frame.render_widget(block("Title", focused), frame.area()))
                .unwrap();
            let buffer = terminal.backend().buffer();
            assert_eq!(buffer[(1, 0)].modifier.contains(Modifier::BOLD), focused);
            assert!(!buffer[(0, 0)].modifier.contains(Modifier::BOLD));
            assert!(!buffer[(1, 1)].modifier.contains(Modifier::BOLD));
        }
    }

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
    fn confirmation_dialogs_fit_their_text_with_minimal_padding() {
        let text = "Delete this comment from the review?\n\ny/Enter: yes     n/Esc: no";
        for area in [Rect::new(0, 0, 120, 40), Rect::new(5, 3, 200, 80)] {
            let (popup, _) = confirmation_layout(area, " Confirm ", text);
            assert_eq!(
                popup.width,
                text.lines().map(UnicodeWidthStr::width).max().unwrap() as u16 + 4
            );
            assert_eq!(popup.height, 5);
            assert_eq!(popup.x, area.x + (area.width - popup.width) / 2);
            assert_eq!(popup.y, area.y + (area.height - popup.height) / 2);
        }
        let text = "A longer confirmation message with enough words to wrap without making the dialog as wide as the terminal.\n\ny/Enter: yes     n/Esc: no";
        let (popup, _) = confirmation_layout(Rect::new(0, 0, 200, 80), " Confirm ", text);
        assert_eq!(popup.width, 64);
        assert_eq!(popup.height, 6);
    }

    #[test]
    fn every_confirmation_keeps_its_choices_visible_on_small_terminals() {
        let mut app = App::new(Snapshot {
            root: "/repo".into(),
            head: "abc".into(),
            files: vec![],
        });
        let mut ui = Ui::default();
        for (width, height) in [(45, 12), (60, 20), (120, 40)] {
            for confirmation in [
                Confirmation::Send,
                Confirmation::Stale,
                Confirmation::Discard,
                Confirmation::Delete(0),
            ] {
                app.mode = Mode::Confirm(confirmation);
                let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
                terminal.draw(|frame| ui.draw(frame, &mut app)).unwrap();
                let buffer = terminal.backend().buffer();
                let text: String = (0..height)
                    .flat_map(|y| (0..width).map(move |x| buffer[(x, y)].symbol()))
                    .collect();
                assert!(
                    text.contains("y/Enter: yes     n/Esc: no"),
                    "Confirmation choices clipped at {width}x{height}"
                );
            }
        }
    }

    #[test]
    fn comment_dialog_starts_small_and_grows_with_its_content() {
        for area in [Rect::new(0, 0, 120, 40), Rect::new(5, 3, 200, 80)] {
            for text in ["", "A short comment"] {
                let popup = comment_layout(area, &Editor::new(text));
                assert_eq!((popup.width, popup.height), (64, 7));
                assert_eq!(popup.x, area.x + (area.width - popup.width) / 2);
                assert_eq!(popup.y, area.y + (area.height - popup.height) / 2);
            }
            let multiline = Editor::new("one\ntwo\nthree\nfour\nfive\nsix");
            assert_eq!(comment_layout(area, &multiline).height, 10);
            let wrapped = Editor::new(&"界".repeat(100));
            assert_eq!(comment_layout(area, &wrapped).height, 8);
            let long = Editor::new(&"line\n".repeat(30));
            assert_eq!(comment_layout(area, &long).height, 14);
        }
    }

    #[test]
    fn compact_comment_dialog_keeps_cursor_and_hints_visible() {
        let mut editor = Editor::new(&format!("{}\nLast line", "界 comment\n".repeat(30)));
        for (width, height) in [(45, 12), (60, 20), (120, 40)] {
            for cursor in [0, editor.chars.len() / 2, editor.chars.len()] {
                editor.cursor = cursor;
                let area = Rect::new(0, 0, width, height);
                let popup = comment_layout(area, &editor);
                assert!(popup.width <= width - 2 && popup.height <= height - 2);
                let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
                terminal
                    .draw(|frame| editor_popup(frame, popup, " Comment ", &editor, COMMENT_HINT))
                    .unwrap();
                let cursor = terminal.get_cursor_position().unwrap();
                // Borders and the two hint rows are outside the editing area.
                assert!(cursor.x > popup.x && cursor.x < popup.right() - 1);
                assert!(cursor.y > popup.y && cursor.y < popup.bottom() - 3);
                let buffer = terminal.backend().buffer();
                let text: String = (0..height)
                    .flat_map(|y| (0..width).map(move |x| buffer[(x, y)].symbol()))
                    .collect();
                assert!(text.contains("Enter/Ctrl-s: keep   Esc: discard"));
                assert!(text.contains("Shift-Enter/Ctrl-j: newline"));
            }
        }
    }

    #[test]
    fn file_list_shows_changes_and_comments_without_review_tracking() {
        use crate::review::{Anchor, Comment};
        use crossterm::event::{KeyCode, KeyModifiers};
        let mut app = navigation_app();
        app.comments.push(Comment {
            anchor: Anchor::file(&app.snapshot.files[0]),
            body: "Please fix".into(),
        });
        let mut ui = Ui::default();
        let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
        let screen_text = |terminal: &Terminal<TestBackend>| -> String {
            let buffer = terminal.backend().buffer();
            (0..40)
                .flat_map(|y| (0..120).map(move |x| buffer[(x, y)].symbol()))
                .collect()
        };
        for files_focused in [false, true] {
            app.files_focused = files_focused;
            terminal.draw(|frame| ui.draw(frame, &mut app)).unwrap();
            let text = screen_text(&terminal);
            assert!(text.contains("2 files  |  1 comments"));
            assert!(text.contains("A a.txt (1)"));
            assert!(!text.contains("[ ]") && !text.contains("[x]"));
            assert!(!text.contains("reviewed"));
            let before = terminal.backend().buffer().clone();
            navigate(&mut app, KeyCode::Char('r'), KeyModifiers::NONE);
            terminal.draw(|frame| ui.draw(frame, &mut app)).unwrap();
            assert_eq!(*terminal.backend().buffer(), before);
        }
        app.mode = Mode::Confirm(Confirmation::Send);
        terminal.draw(|frame| ui.draw(frame, &mut app)).unwrap();
        let text = screen_text(&terminal);
        assert!(text.contains("Send 1 comment(s) to the agent and close?"));
        assert!(!text.contains("reviewed"));
        assert!(!HELP.contains("Toggle file reviewed"));
    }

    fn navigation_app() -> App {
        let files = [("a.txt", [25, 30]), ("b.txt", [40, 80])]
            .into_iter()
            .map(|(path, matches)| {
                let mut patch = String::from("@@ -0,0 +1,100 @@\n");
                for line in 1..=100 {
                    patch.push_str(&format!(
                        "+{} {line}\n",
                        if matches.contains(&line) {
                            "needle"
                        } else {
                            "line"
                        }
                    ));
                }
                FileDiff {
                    path: path.into(),
                    status: 'A',
                    rows: parse_patch(&patch).unwrap(),
                    patch: patch.into_bytes(),
                }
            })
            .collect();
        App::new(Snapshot {
            root: "/repo".into(),
            head: "abc".into(),
            files,
        })
    }

    fn navigate(
        app: &mut App,
        code: crossterm::event::KeyCode,
        modifiers: crossterm::event::KeyModifiers,
    ) {
        app.event(crossterm::event::Event::Key(
            crossterm::event::KeyEvent::new(code, modifiers),
        ));
    }

    #[test]
    fn paging_recenters_once_while_line_movement_keeps_the_viewport() {
        use crossterm::event::{KeyCode, KeyModifiers};
        let mut app = navigation_app();
        let mut ui = Ui::default();
        let mut terminal = Terminal::new(TestBackend::new(120, 25)).unwrap();
        app.views[0].row = 35;
        app.views[0].top = 30;
        app.views[0].left = 4;
        app.selection = Some(35);
        terminal.draw(|frame| ui.draw(frame, &mut app)).unwrap();
        assert_eq!(app.height, 20);
        for (key, modifiers, row) in [
            (KeyCode::Char('d'), KeyModifiers::CONTROL, 45),
            (KeyCode::Char('u'), KeyModifiers::CONTROL, 35),
            (KeyCode::PageDown, KeyModifiers::NONE, 55),
            (KeyCode::PageUp, KeyModifiers::NONE, 35),
        ] {
            navigate(&mut app, key, modifiers);
            terminal.draw(|frame| ui.draw(frame, &mut app)).unwrap();
            assert_eq!(app.views[0].row, row);
            assert_eq!(app.views[0].top, row - 10);
            assert!(!app.views[0].center);
            assert_eq!(app.views[0].left, 4);
            assert_eq!(app.selection, Some(35));
            // The marker is physically at the middle of the diff's inner area.
            assert_eq!(terminal.backend().buffer()[(31, 12)].symbol(), ">");
        }
        for key in [
            KeyCode::Char('j'),
            KeyCode::Char('k'),
            KeyCode::Down,
            KeyCode::Up,
        ] {
            navigate(&mut app, key, KeyModifiers::NONE);
            terminal.draw(|frame| ui.draw(frame, &mut app)).unwrap();
            assert_eq!(app.views[0].top, 25);
        }
    }

    #[test]
    fn centering_handles_file_boundaries_and_uses_the_new_size_after_resize() {
        use crossterm::event::{KeyCode, KeyModifiers};
        let mut app = navigation_app();
        let mut ui = Ui::default();
        let mut terminal = Terminal::new(TestBackend::new(120, 25)).unwrap();
        terminal.draw(|frame| ui.draw(frame, &mut app)).unwrap();
        navigate(&mut app, KeyCode::Char('u'), KeyModifiers::CONTROL);
        terminal.draw(|frame| ui.draw(frame, &mut app)).unwrap();
        assert_eq!((app.views[0].row, app.views[0].top), (0, 0));
        app.views[0].row = 95;
        navigate(&mut app, KeyCode::Char('d'), KeyModifiers::CONTROL);
        terminal.draw(|frame| ui.draw(frame, &mut app)).unwrap();
        assert_eq!((app.views[0].row, app.views[0].top), (100, 90));

        app.views[0].row = 35;
        navigate(&mut app, KeyCode::Char('d'), KeyModifiers::CONTROL);
        let mut terminal = Terminal::new(TestBackend::new(120, 35)).unwrap();
        terminal.draw(|frame| ui.draw(frame, &mut app)).unwrap();
        assert_eq!(app.height, 30);
        assert_eq!((app.views[0].row, app.views[0].top), (45, 30));
    }

    #[test]
    fn hunk_jumps_center_in_both_directions_including_visible_and_final_hunks() {
        use crossterm::event::{KeyCode, KeyModifiers};
        let mut app = navigation_app();
        let mut patch = String::new();
        let mut added = 0;
        for (i, count) in [20, 4, 40, 1].into_iter().enumerate() {
            let old = i * 100 + 1;
            patch.push_str(&format!("@@ -{old},0 +{},{} @@\n", old + added, count));
            for line in 0..count {
                patch.push_str(&format!("+line {line}\n"));
            }
            added += count;
        }
        app.snapshot.files[0].rows = parse_patch(&patch).unwrap();
        app.snapshot.files[0].patch = patch.into_bytes();
        let mut ui = Ui::default();
        let mut terminal = Terminal::new(TestBackend::new(120, 25)).unwrap();
        terminal.draw(|frame| ui.draw(frame, &mut app)).unwrap();
        for (key, row) in [
            (']', 21),
            (']', 26),
            (']', 67),
            (']', 67),
            ('[', 26),
            ('[', 21),
            ('[', 0),
            ('[', 0),
        ] {
            navigate(&mut app, KeyCode::Char(key), KeyModifiers::NONE);
            terminal.draw(|frame| ui.draw(frame, &mut app)).unwrap();
            let view = app.views[0];
            assert_eq!(view.row, row);
            assert_eq!(view.top, row.saturating_sub(10));
            assert!(!view.center);
            assert_eq!(
                terminal.backend().buffer()[(31, 2 + (row - view.top) as u16)].symbol(),
                ">"
            );
        }
    }

    #[test]
    fn search_centers_visible_matches_cross_file_jumps_and_wraparound() {
        use crossterm::event::{Event, KeyCode, KeyModifiers};
        let mut app = navigation_app();
        let mut ui = Ui::default();
        let mut terminal = Terminal::new(TestBackend::new(120, 25)).unwrap();
        terminal.draw(|frame| ui.draw(frame, &mut app)).unwrap();
        navigate(&mut app, KeyCode::Char('/'), KeyModifiers::NONE);
        app.event(Event::Paste("needle".into()));
        for (key, file, row) in [
            (KeyCode::Enter, 0, 25),
            (KeyCode::Char('n'), 0, 30),
            (KeyCode::Char('n'), 1, 40),
            (KeyCode::Char('N'), 0, 30),
            (KeyCode::Char('N'), 0, 25),
            (KeyCode::Char('N'), 1, 80),
            (KeyCode::Char('n'), 0, 25),
        ] {
            navigate(&mut app, key, KeyModifiers::NONE);
            terminal.draw(|frame| ui.draw(frame, &mut app)).unwrap();
            assert_eq!(app.file, file);
            assert_eq!(app.views[file].row, row);
            assert_eq!(app.views[file].top, row - 10);
            assert_eq!(terminal.backend().buffer()[(31, 12)].symbol(), ">");
        }
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
