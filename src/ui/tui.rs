//! The Sublime-Text-style picker: typed query on the top line, a dim rule
//! under it (which carries the protocol warning when there is one),
//! fuzzy-filtered results underneath, matched characters bold, keybinding
//! hints right-aligned and dim, current selection a full-width reversed bar.
//! Terminal-default colors only — the popup follows whatever theme the
//! terminal already has.

use std::io::{self, Stdout};

use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use nucleo_matcher::pattern::{CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher, Utf32Str};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::Position;
use ratatui::style::{Color, Modifier, Style};
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
    let query_span = if query.is_empty() && !screen.placeholder.is_empty() {
        Span::styled(
            screen.placeholder.clone(),
            Style::default().add_modifier(Modifier::DIM),
        )
    } else {
        Span::raw(query.to_string())
    };
    lines.push(Line::from(vec![
        Span::styled(
            screen.prompt.clone(),
            Style::default().add_modifier(Modifier::BOLD),
        ),
        query_span,
    ]));
    lines.push(header_line(&screen.header, screen.warning, width));

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
        let hint_len = row.hint.chars().count();
        if hint_len > 0 && shown + 2 + hint_len <= width {
            // Right-align the keybinding hint, dim over the row's base
            // style; a row too narrow for label + hint drops the hint.
            spans.push(Span::styled(" ".repeat(width - shown - hint_len), base));
            let (faded, key) = split_hint(&row.hint);
            if faded.is_empty() {
                spans.push(Span::styled(
                    key.to_string(),
                    base.add_modifier(Modifier::DIM),
                ));
            } else {
                spans.push(Span::styled(
                    faded.to_string(),
                    base.add_modifier(Modifier::DIM),
                ));
                spans.push(Span::styled(key.to_string(), base));
            }
        } else if visible_index == selected && width > shown {
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

/// Splits a keybinding hint into the leading `prefix+` and the rest. That
/// prefix repeats on nearly every catalog row, so it is faded out while the
/// part that actually names the key keeps the base style. A hint that does
/// not start with it has nothing to fade and is returned whole.
fn split_hint(hint: &str) -> (&str, &str) {
    match hint.split_once('+') {
        Some(("prefix", rest)) => (&hint[..PREFIX.len()], rest),
        _ => ("", hint),
    }
}

const PREFIX: &str = "prefix+";

/// The rule separating the query from the results doubles as the warning
/// channel: an empty header renders as a bare full-width rule, anything else
/// is embedded in it (`── warning… ───`).
fn rule_line(header: &str, width: usize) -> String {
    let (text, padding) = rule_parts(header, width);
    text + &padding
}

/// The embedded header and the rule that pads it to `width`, kept apart so
/// they can be styled differently.
fn rule_parts(header: &str, width: usize) -> (String, String) {
    let text = if header.is_empty() {
        String::new()
    } else {
        format!("── {header} ")
    };
    let shown = text.chars().count();
    let padding = if width > shown {
        "─".repeat(width - shown)
    } else {
        String::new()
    };
    (text, padding)
}

/// Marks a header rendered as a warning. U+26A0 without the emoji variation
/// selector, so most terminals keep it to one cell.
const WARNING_MARK: &str = "⚠ ";

/// A description is chrome and stays dim; a warning is the one thing on the
/// screen the user must not miss, so it gets bold and the terminal's own
/// yellow — a palette colour, not an RGB one, so the theme decides its shade.
fn header_line(header: &str, warning: bool, width: usize) -> Line<'static> {
    let dim = Style::default().add_modifier(Modifier::DIM);
    if !warning || header.is_empty() {
        return Line::from(Span::styled(rule_line(header, width), dim));
    }
    let (text, padding) = rule_parts(&format!("{WARNING_MARK}{header}"), width);
    Line::from(vec![
        Span::styled(
            text,
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(padding, dim),
    ])
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
                    let mut lines = vec![Line::from(vec![
                        Span::styled(
                            screen.prompt.clone(),
                            Style::default().add_modifier(Modifier::BOLD),
                        ),
                        Span::raw(text),
                    ])];
                    if !screen.header.is_empty() {
                        lines.push(Line::from(Span::styled(
                            screen.header.clone(),
                            Style::default().add_modifier(Modifier::DIM),
                        )));
                    }
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

#[cfg(test)]
mod tests {
    #[test]
    fn a_prefixed_hint_fades_only_its_prefix() {
        assert_eq!(super::split_hint("prefix+shift+f"), ("prefix+", "shift+f"));
    }

    #[test]
    fn a_hint_without_the_prefix_stays_whole() {
        assert_eq!(super::split_hint("ctrl+alt+z"), ("", "ctrl+alt+z"));
    }

    #[test]
    fn a_key_merely_starting_with_the_prefix_letters_stays_whole() {
        assert_eq!(super::split_hint("prefixed+p"), ("", "prefixed+p"));
    }

    use super::rule_line;

    #[test]
    fn an_empty_header_renders_as_a_bare_full_width_rule() {
        assert_eq!(rule_line("", 5), "─────");
    }

    #[test]
    fn a_warning_is_embedded_in_the_rule_and_padded_to_width() {
        assert_eq!(rule_line("boom", 12), "── boom ────");
    }

    #[test]
    fn a_header_wider_than_the_popup_is_kept_intact() {
        assert_eq!(rule_line("boom", 5), "── boom ");
    }

    #[test]
    fn zero_width_yields_an_empty_rule() {
        assert_eq!(rule_line("", 0), "");
    }
}
