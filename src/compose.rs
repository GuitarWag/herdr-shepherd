use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    Frame,
    layout::{Constraint, Flex, Layout, Rect},
    style::{Color, Style, Stylize},
    text::{Line, Span},
    widgets::{Block, BorderType, Clear, Padding, Paragraph, Wrap},
};

const ACCENT: Color = Color::Cyan;
const MUTED: Color = Color::DarkGray;

/// A card over the board: a message to write, or a message to read.
pub enum Modal {
    Compose {
        to: String,
        text: String,
        error: String,
    },
    Message {
        id: String,
        body: String,
        scroll: u16,
    },
}

pub enum Outcome {
    Close,
    Send { to: String, text: String },
}

fn card(frame: &mut Frame, title: String, hints: &[(&str, &str)], height: u16) -> Rect {
    let area = frame.area();
    let [card] = Layout::horizontal([Constraint::Length(area.width.saturating_sub(4).min(80))])
        .flex(Flex::Center)
        .areas(area);
    let [card] = Layout::vertical([Constraint::Length(height.min(area.height))])
        .flex(Flex::Center)
        .areas(card);
    let mut line = Vec::new();
    for (key, label) in hints {
        line.push(Span::styled(
            format!(" {key}"),
            Style::new().fg(ACCENT).bold(),
        ));
        line.push(Span::styled(format!(" {label} "), Style::new().fg(MUTED)));
    }
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(ACCENT))
        .title(Span::styled(title, Style::new().fg(ACCENT).bold()))
        .title_bottom(Line::from(line))
        .padding(Padding::new(1, 1, 1, 0));
    let inner = block.inner(card);
    frame.render_widget(Clear, card);
    frame.render_widget(block, card);
    inner
}

impl Modal {
    pub fn compose(to: &str) -> Self {
        Self::Compose {
            to: to.to_owned(),
            text: String::new(),
            error: String::new(),
        }
    }

    pub fn insert(&mut self, value: &str) {
        if let Self::Compose { text, error, .. } = self {
            let value = value.replace("\r\n", "\n").replace('\r', "\n");
            text.extend(value.chars().filter(|c| *c == '\n' || !c.is_control()));
            error.clear();
        }
    }

    pub fn fail(&mut self, message: String) {
        if let Self::Compose { error, .. } = self {
            *error = message;
        }
    }

    pub fn key(&mut self, key: KeyEvent) -> Option<Outcome> {
        let control = key.modifiers.contains(KeyModifiers::CONTROL);
        if key.code == KeyCode::Esc || key.code == KeyCode::Char('c') && control {
            return Some(Outcome::Close);
        }
        match self {
            Self::Compose { to, text, error } => match key.code {
                KeyCode::Enter if key.modifiers.contains(KeyModifiers::ALT) => text.push('\n'),
                KeyCode::Char('j') if control => text.push('\n'),
                KeyCode::Enter if text.trim().is_empty() => *error = "The message is empty.".into(),
                KeyCode::Enter => {
                    return Some(Outcome::Send {
                        to: to.clone(),
                        text: text.clone(),
                    });
                }
                KeyCode::Backspace => {
                    text.pop();
                }
                KeyCode::Char('u') if control => text.clear(),
                KeyCode::Char(c) if !control => {
                    text.push(c);
                    error.clear();
                }
                _ => {}
            },
            Self::Message { scroll, .. } => match key.code {
                KeyCode::Char('q') | KeyCode::Enter => return Some(Outcome::Close),
                KeyCode::Down | KeyCode::Char('j') => *scroll = scroll.saturating_add(1),
                KeyCode::Up | KeyCode::Char('k') => *scroll = scroll.saturating_sub(1),
                _ => {}
            },
        }
        None
    }

    pub fn draw(&self, frame: &mut Frame) {
        match self {
            Self::Compose { to, text, error } => {
                let inner = card(
                    frame,
                    format!(" message to {to} "),
                    &[("⏎", "send"), ("ctrl+j", "new line"), ("esc", "cancel")],
                    14,
                );
                let [body, status_line] =
                    Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).areas(inner);
                let lines: Vec<Line> = if text.is_empty() {
                    vec![
                        Line::from("Write the task or the reply.")
                            .fg(MUTED)
                            .italic(),
                    ]
                } else {
                    text.split('\n').map(Line::raw).collect()
                };
                // Keep the end of a long message, where the cursor is.
                let height = body.height as usize;
                let start = lines.len().saturating_sub(height);
                let cursor_line = lines.len().min(height).saturating_sub(1) as u16;
                let column = if text.is_empty() {
                    0
                } else {
                    lines.last().map_or(0, Line::width) as u16
                };
                frame.render_widget(Paragraph::new(lines[start..].to_vec()), body);
                frame.set_cursor_position((
                    body.x + column.min(body.width.saturating_sub(1)),
                    body.y + cursor_line,
                ));
                let status = if error.is_empty() {
                    Paragraph::new("The agent receives a one-line pointer to this message.")
                        .fg(MUTED)
                } else {
                    Paragraph::new(error.as_str()).fg(Color::Red)
                };
                frame.render_widget(status, status_line);
            }
            Self::Message { id, body, scroll } => {
                let height = frame.area().height.saturating_sub(4).min(24);
                let inner = card(
                    frame,
                    format!(" {id} "),
                    &[("↑↓", "scroll"), ("esc", "close")],
                    height,
                );
                frame.render_widget(
                    Paragraph::new(body.as_str())
                        .wrap(Wrap { trim: false })
                        .scroll((*scroll, 0)),
                    inner,
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{Terminal, backend::TestBackend};

    fn key(modal: &mut Modal, code: KeyCode, modifiers: KeyModifiers) -> Option<Outcome> {
        modal.key(KeyEvent::new(code, modifiers))
    }

    fn screen(modal: &Modal) -> String {
        let mut terminal = Terminal::new(TestBackend::new(90, 30)).unwrap();
        terminal.draw(|frame| modal.draw(frame)).unwrap();
        terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect()
    }

    #[test]
    fn compose_keeps_lines_and_rejects_an_empty_message() {
        let mut modal = Modal::compose("auth-boss");
        assert!(key(&mut modal, KeyCode::Enter, KeyModifiers::NONE).is_none());
        assert!(screen(&modal).contains("The message is empty."));
        modal.insert("first\r\nsecond\x1b");
        key(&mut modal, KeyCode::Char('j'), KeyModifiers::CONTROL);
        key(&mut modal, KeyCode::Char('x'), KeyModifiers::NONE);
        key(&mut modal, KeyCode::Backspace, KeyModifiers::NONE);
        key(&mut modal, KeyCode::Char('3'), KeyModifiers::NONE);
        let text = screen(&modal);
        assert!(text.contains("message to auth-boss"));
        assert!(text.contains("second"));
        match key(&mut modal, KeyCode::Enter, KeyModifiers::NONE) {
            Some(Outcome::Send { to, text }) => {
                assert_eq!(to, "auth-boss");
                assert_eq!(text, "first\nsecond\n3");
            }
            _ => panic!("Enter did not send"),
        }
        assert!(matches!(
            key(&mut modal, KeyCode::Esc, KeyModifiers::NONE),
            Some(Outcome::Close)
        ));
    }

    #[test]
    fn message_view_scrolls_and_closes() {
        let mut modal = Modal::Message {
            id: "m1".into(),
            body: "From: org-director\nTo: auth-boss\n\nUse the cache.".into(),
            scroll: 0,
        };
        assert!(screen(&modal).contains("Use the cache."));
        key(&mut modal, KeyCode::Down, KeyModifiers::NONE);
        key(&mut modal, KeyCode::Down, KeyModifiers::NONE);
        key(&mut modal, KeyCode::Up, KeyModifiers::NONE);
        assert!(matches!(modal, Modal::Message { scroll: 1, .. }));
        assert!(matches!(
            key(&mut modal, KeyCode::Char('q'), KeyModifiers::NONE),
            Some(Outcome::Close)
        ));
    }
}
