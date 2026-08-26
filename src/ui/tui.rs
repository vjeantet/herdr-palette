//! The Sublime-Text-style picker: typed query on the top line, fuzzy-filtered
//! results underneath, matched characters bold, current selection a
//! full-width reversed bar. Terminal-default colors only — the popup follows
//! whatever theme the terminal already has.

use std::io::{self, Stdout};

use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use nucleo_matcher::pattern::{CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher, Utf32Str};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::Position;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Paragraph, Wrap};
use ratatui::{Frame, Terminal};

use super::{InputOutcome, InputScreen, PickOutcome, PickScreen, Ui};
use crate::fatal::Fatal;

pub struct TuiUi {
    terminal: Terminal<CrosstermBackend<Stdout>>,
    matcher: Matcher,
    restored: bool,
}

fn term_err(err: impl std::fmt::Display) -> Fatal {
    Fatal(format!("command-palette: terminal error: {err}"))
}

impl TuiUi {
    pub fn new() -> Result<Self, Fatal> {
        enable_raw_mode().map_err(term_err)?;
        let mut stdout = io::stdout();
        crossterm::execute!(stdout, EnterAlternateScreen).map_err(term_err)?;
        let terminal = Terminal::new(CrosstermBackend::new(stdout)).map_err(term_err)?;
        Ok(TuiUi {
            terminal,
            matcher: Matcher::new(Config::DEFAULT),
            restored: false,
        })
    }

    fn restore(&mut self) {
        if self.restored {
            return;
        }
        self.restored = true;
        let _ = disable_raw_mode();
        let _ = crossterm::execute!(io::stdout(), LeaveAlternateScreen);
    }
}

impl Drop for TuiUi {
    fn drop(&mut self) {
        self.restore();
    }
}

/// One visible row: index into `screen.rows` plus the matched character
/// positions (char indices, sorted, deduplicated).
struct Filtered {
    row: usize,
    indices: Vec<u32>,
}

fn filter_rows(matcher: &mut Matcher, screen: &PickScreen, query: &str) -> Vec<Filtered> {
    if query.is_empty() {
        // No query: full list in catalog order, nothing highlighted.
        return (0..screen.rows.len())
            .map(|row| Filtered {
                row,
                indices: Vec::new(),
            })
            .collect();
    }
    let pattern = Pattern::parse(query, CaseMatching::Ignore, Normalization::Smart);
    let mut buf = Vec::new();
    let mut scored: Vec<(u32, usize, Vec<u32>)> = Vec::new();
    for (row, entry) in screen.rows.iter().enumerate() {
        let mut indices = Vec::new();
        let haystack = Utf32Str::new(&entry.label, &mut buf);
        if let Some(score) = pattern.indices(haystack, matcher, &mut indices) {
            indices.sort_unstable();
            indices.dedup();
            scored.push((score, row, indices));
        }
    }
    // Score descending, stable by original (catalog) order.
    scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    scored
        .into_iter()
        .map(|(_, row, indices)| Filtered { row, indices })
        .collect()
}

fn render_pick(
    frame: &mut Frame,
    screen: &PickScreen,
    query: &str,
    filtered: &[Filtered],
    selected: usize,
    offset: &mut usize,
) {
    let area = frame.area();
    let width = area.width as usize;
    let list_height = area.height.saturating_sub(2) as usize;

    // Keep the selection visible.
    if selected < *offset {
        *offset = selected;
    }
    if list_height > 0 && selected >= *offset + list_height {
        *offset = selected - list_height + 1;
    }

    let mut lines: Vec<Line> = Vec::with_capacity(2 + list_height);
    lines.push(Line::from(vec![
        Span::styled(
            screen.prompt.clone(),
            Style::default().add_modifier(Modifier::BOLD),
        ),
        Span::raw(query.to_string()),
    ]));
    lines.push(Line::from(Span::styled(
        screen.header.clone(),
        Style::default().add_modifier(Modifier::DIM),
    )));

    for (visible_index, entry) in filtered.iter().enumerate().skip(*offset).take(list_height) {
        let row = &screen.rows[entry.row];
        let base = if visible_index == selected {
            Style::default().add_modifier(Modifier::REVERSED)
        } else {
            Style::default()
        };
        let mut spans: Vec<Span> = Vec::new();
        let mut shown = 0usize;
        for (char_index, ch) in row.label.chars().enumerate() {
            let mut style = base;
            if entry.indices.binary_search(&(char_index as u32)).is_ok() {
                style = style.add_modifier(Modifier::BOLD);
            }
            spans.push(Span::styled(ch.to_string(), style));
            shown += 1;
        }
        if visible_index == selected && width > shown {
            // Pad the selected row so the reversed bar spans the full width.
            spans.push(Span::styled(" ".repeat(width - shown), base));
        }
        lines.push(Line::from(spans));
    }

    frame.render_widget(Paragraph::new(Text::from(lines)), area);
    let cursor_x =
        (screen.prompt.chars().count() + query.chars().count()).min(width.saturating_sub(1));
    frame.set_cursor_position(Position::new(cursor_x as u16, area.y));
}

impl Ui for TuiUi {
    fn pick(&mut self, screen: &PickScreen) -> Result<PickOutcome, Fatal> {
        let mut query = String::new();
        let mut selected = 0usize;
        let mut offset = 0usize;
        loop {
            let filtered = filter_rows(&mut self.matcher, screen, &query);
            if selected >= filtered.len() {
                selected = filtered.len().saturating_sub(1);
            }
            self.terminal
                .draw(|frame| render_pick(frame, screen, &query, &filtered, selected, &mut offset))
                .map_err(term_err)?;
            let Event::Key(key) = event::read().map_err(term_err)? else {
                continue; // resize etc.: redraw
            };
            if key.kind != KeyEventKind::Press {
                continue;
            }
            let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
            match (key.code, ctrl) {
                (KeyCode::Esc, _) | (KeyCode::Char('c'), true) => {
                    return Ok(PickOutcome::Cancelled)
                }
                (KeyCode::Enter, _) => {
                    // Enter with nothing to accept is a clean cancel, as
                    // fzf's rc 1 (no match) was.
                    return Ok(match filtered.get(selected) {
                        Some(entry) => PickOutcome::Selected(screen.rows[entry.row].id.clone()),
                        None => PickOutcome::Cancelled,
                    });
                }
                (KeyCode::Up, _) | (KeyCode::Char('k'), true) | (KeyCode::Char('p'), true) => {
                    selected = selected.saturating_sub(1);
                }
                (KeyCode::Down, _) | (KeyCode::Char('j'), true) | (KeyCode::Char('n'), true) => {
                    if selected + 1 < filtered.len() {
                        selected += 1;
                    }
                }
                (KeyCode::Char('u'), true) => {
                    query.clear();
                    selected = 0;
                }
                (KeyCode::Backspace, _) => {
                    query.pop();
                    selected = 0;
                }
                (KeyCode::Char(ch), false) => {
                    query.push(ch);
                    selected = 0;
                }
                _ => {}
            }
        }
    }

    fn input(&mut self, screen: &InputScreen) -> Result<InputOutcome, Fatal> {
        // A plain line editor in the picker's clothes: same top line, same
        // dim header, no result list.
        let mut value: Vec<char> = screen.initial.chars().collect();
        let mut cursor = value.len();
        loop {
            self.terminal
                .draw(|frame| {
                    let area = frame.area();
                    let text: String = value.iter().collect();
                    let lines = vec![
                        Line::from(vec![
                            Span::styled(
                                screen.prompt.clone(),
                                Style::default().add_modifier(Modifier::BOLD),
                            ),
                            Span::raw(text),
                        ]),
                        Line::from(Span::styled(
                            screen.header.clone(),
                            Style::default().add_modifier(Modifier::DIM),
                        )),
                    ];
                    frame.render_widget(Paragraph::new(Text::from(lines)), area);
                    let cursor_x = (screen.prompt.chars().count() + cursor)
                        .min((area.width as usize).saturating_sub(1));
                    frame.set_cursor_position(Position::new(cursor_x as u16, area.y));
                })
                .map_err(term_err)?;
            let Event::Key(key) = event::read().map_err(term_err)? else {
                continue;
            };
            if key.kind != KeyEventKind::Press {
                continue;
            }
            let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
            match (key.code, ctrl) {
                (KeyCode::Esc, _) | (KeyCode::Char('c'), true) => {
                    return Ok(InputOutcome::Cancelled)
                }
                (KeyCode::Enter, _) => return Ok(InputOutcome::Submitted(value.iter().collect())),
                (KeyCode::Left, _) | (KeyCode::Char('b'), true) => {
                    cursor = cursor.saturating_sub(1);
                }
                (KeyCode::Right, _) | (KeyCode::Char('f'), true) => {
                    cursor = (cursor + 1).min(value.len());
                }
                (KeyCode::Home, _) | (KeyCode::Char('a'), true) => cursor = 0,
                (KeyCode::End, _) | (KeyCode::Char('e'), true) => cursor = value.len(),
                (KeyCode::Char('u'), true) => {
                    value.clear();
                    cursor = 0;
                }
                (KeyCode::Backspace, _) => {
                    if cursor > 0 {
                        cursor -= 1;
                        value.remove(cursor);
                    }
                }
                (KeyCode::Delete, _) => {
                    if cursor < value.len() {
                        value.remove(cursor);
                    }
                }
                (KeyCode::Char(ch), false) => {
                    value.insert(cursor, ch);
                    cursor += 1;
                }
                _ => {}
            }
        }
    }

    fn fatal(&mut self, message: &str) -> ! {
        // The popup closes with the process; without this pause the message
        // would vanish before it could be read (the bash `die` behaved the
        // same way).
        let mut lines: Vec<Line> = message.lines().map(Line::from).collect();
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "(press any key to close)",
            Style::default().add_modifier(Modifier::DIM),
        )));
        let paragraph = Paragraph::new(Text::from(lines)).wrap(Wrap { trim: false });
        loop {
            if self
                .terminal
                .draw(|frame| frame.render_widget(&paragraph, frame.area()))
                .is_err()
            {
                break;
            }
            match event::read() {
                Ok(Event::Key(key)) if key.kind == KeyEventKind::Press => break,
                Ok(_) => continue,
                Err(_) => break,
            }
        }
        self.restore();
        eprintln!("{message}");
        std::process::exit(1);
    }
}
