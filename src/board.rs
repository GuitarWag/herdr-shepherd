use crate::{
    Result,
    launcher::{Launcher, Outcome, PasteMode},
    optional, process,
};
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::{
    Frame,
    layout::{Constraint, Flex, Layout, Rect},
    style::{Color, Modifier, Style, Stylize},
    text::{Line, Span},
    widgets::{Block, BorderType, List, ListItem, ListState, Padding, Paragraph},
};
use serde_json::Value;
use std::{
    collections::BTreeSet,
    process::Command,
    time::{Duration, Instant},
};

const ACCENT: Color = Color::Cyan;
const MUTED: Color = Color::DarkGray;
const MIN_WIDTH: u16 = 60;
const MIN_HEIGHT: u16 = 14;
const REFRESH: Duration = Duration::from_secs(2);

pub enum Action {
    Quit,
    Launch(Box<crate::cli::Options>),
}

/// The task names of the directors in a group list.
pub fn directors(groups: &[Value]) -> Vec<String> {
    groups
        .iter()
        .filter(|group| is_director(group))
        .map(|group| optional(group, "subject").to_owned())
        .collect()
}

/// One line of the tree: a group, or one member of a group.
#[derive(Clone, Copy)]
struct Row {
    depth: usize,
    group: usize,
    member: Option<usize>,
}

#[derive(Default)]
pub struct Board {
    groups: Vec<Value>,
    rows: Vec<Row>,
    list: ListState,
    collapsed: BTreeSet<String>,
    pane: String,
    inbox: Vec<(String, String, String)>,
    message: String,
    /// The selection changed, so the detail panel needs new data.
    stale: bool,
    launcher: Option<Launcher>,
}

fn members(group: &Value) -> &[Value] {
    group["members"].as_array().map_or(&[], Vec::as_slice)
}

fn is_director(group: &Value) -> bool {
    members(group)
        .iter()
        .any(|member| optional(member, "role") == "director")
}

/// A session with one agent shows on one row.
fn single(group: &Value) -> bool {
    optional(group, "kind") != "herd" && members(group).len() == 1
}

fn status_style(status: &str) -> (&'static str, Color) {
    match status {
        "idle" => ("●", Color::Green),
        "working" => ("◐", Color::Yellow),
        "blocked" => ("●", Color::Red),
        "done" => ("●", Color::Cyan),
        _ => ("○", MUTED),
    }
}

impl Board {
    fn key_of(&self, row: Row) -> String {
        let group = &self.groups[row.group];
        match row.member {
            Some(index) => format!(
                "{}/{}",
                optional(group, "subject"),
                optional(&members(group)[index], "name")
            ),
            None => optional(group, "subject").to_owned(),
        }
    }

    pub fn load(&mut self, groups: Vec<Value>) {
        let selected = self.selected_row().map(|row| self.key_of(row));
        self.groups = groups;
        self.rebuild(selected);
    }

    fn rebuild(&mut self, selected: Option<String>) {
        self.build();
        let index = selected
            .and_then(|key| (0..self.rows.len()).find(|&i| self.key_of(self.rows[i]) == key))
            .unwrap_or(0);
        self.list.select((!self.rows.is_empty()).then_some(index));
        self.stale = true;
    }

    fn build(&mut self) {
        self.rows.clear();
        let directors: BTreeSet<&str> = self
            .groups
            .iter()
            .filter(|group| is_director(group))
            .map(|group| optional(group, "subject"))
            .collect();
        let top: Vec<usize> = (0..self.groups.len())
            .filter(|&i| !directors.contains(optional(&self.groups[i], "under")))
            .collect();
        for index in top {
            self.push(index, 0);
        }
    }

    fn push(&mut self, index: usize, depth: usize) {
        self.rows.push(Row {
            depth,
            group: index,
            member: None,
        });
        let group = &self.groups[index];
        let subject = optional(group, "subject").to_owned();
        if self.collapsed.contains(&subject) || single(group) {
            return;
        }
        if is_director(group) {
            let teams: Vec<usize> = (0..self.groups.len())
                .filter(|&i| i != index && optional(&self.groups[i], "under") == subject)
                .collect();
            for team in teams {
                self.push(team, depth + 1);
            }
        } else {
            for member in 0..members(group).len() {
                self.rows.push(Row {
                    depth: depth + 1,
                    group: index,
                    member: Some(member),
                });
            }
        }
    }

    fn selected_row(&self) -> Option<Row> {
        self.list.selected().and_then(|i| self.rows.get(i).copied())
    }

    /// The agent that the detail panel shows: a member, or a group's lead.
    fn agent(&self) -> Option<&Value> {
        let row = self.selected_row()?;
        let list = members(&self.groups[row.group]);
        match row.member {
            Some(index) => list.get(index),
            None => list
                .iter()
                .find(|m| matches!(optional(m, "role"), "director" | "boss"))
                .or(list.first()),
        }
    }

    fn toggle(&mut self, open: bool) {
        let Some(row) = self.selected_row() else {
            return;
        };
        let subject = optional(&self.groups[row.group], "subject").to_owned();
        if row.member.is_some() && !open {
            let parent = (0..self.list.selected().unwrap_or(0))
                .rev()
                .find(|&i| self.rows[i].group == row.group && self.rows[i].member.is_none());
            self.list.select(parent);
        } else if open {
            self.collapsed.remove(&subject);
        } else {
            self.collapsed.insert(subject);
        }
        let selected = self.selected_row().map(|row| self.key_of(row));
        self.rebuild(selected);
    }

    /// The director of the selected row, as a default for a new team.
    fn selected_director(&self) -> Option<String> {
        let group = &self.groups[self.selected_row()?.group];
        if is_director(group) {
            Some(optional(group, "subject").to_owned())
        } else {
            Some(optional(group, "under").to_owned()).filter(|under| !under.is_empty())
        }
    }

    fn open(&mut self, director: bool) -> Result<()> {
        let under = self.selected_director();
        self.launcher = Some(Launcher::new(
            director,
            directors(&self.groups),
            under.as_deref(),
            false,
        )?);
        Ok(())
    }

    fn key(&mut self, key: KeyEvent) -> Result<Option<Action>> {
        if let Some(launcher) = &mut self.launcher {
            match launcher.key(key) {
                Some(Outcome::Cancel) => self.launcher = None,
                Some(Outcome::Launch(options)) => return Ok(Some(Action::Launch(options))),
                None => {}
            }
            return Ok(None);
        }
        self.message.clear();
        let before = self.list.selected();
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => return Ok(Some(Action::Quit)),
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                return Ok(Some(Action::Quit));
            }
            KeyCode::Char('n') => self.open(false)?,
            KeyCode::Char('d') => self.open(true)?,
            KeyCode::Down | KeyCode::Char('j') => self.list.select_next(),
            KeyCode::Up | KeyCode::Char('k') => self.list.select_previous(),
            KeyCode::Home | KeyCode::Char('g') => self.list.select_first(),
            KeyCode::End | KeyCode::Char('G') => self.list.select_last(),
            KeyCode::Left | KeyCode::Char('h') => self.toggle(false),
            KeyCode::Right | KeyCode::Char('l') => self.toggle(true),
            KeyCode::Enter => {
                let tab = self
                    .agent()
                    .map(|agent| optional(agent, "tab_id").to_owned());
                match tab.filter(|tab| !tab.is_empty()) {
                    Some(tab) => {
                        process::herdr(&["tab", "focus", &tab])?;
                    }
                    None => self.message = "This agent has no herdr tab.".into(),
                }
            }
            _ => {}
        }
        if let Some(index) = self.list.selected() {
            self.list
                .select(Some(index.min(self.rows.len().saturating_sub(1))));
        }
        self.stale |= before != self.list.selected();
        Ok(None)
    }

    fn refresh(&mut self) {
        match crate::view::groups(None) {
            Ok(groups) => self.load(groups),
            Err(error) => self.message = error.to_string(),
        }
    }

    /// Read the selected pane and the inbox of the selected agent.
    fn detail(&mut self) {
        self.stale = false;
        let Some(agent) = self.agent() else {
            self.pane.clear();
            self.inbox.clear();
            return;
        };
        let pane = optional(agent, "pane_id").to_owned();
        let name = optional(agent, "name").to_owned();
        self.pane = process::output(
            Command::new("herdr").args(["pane", "read", &pane, "--source", "visible"]),
            Duration::from_secs(2),
        )
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).into_owned())
        .unwrap_or_default();
        self.inbox = crate::view::team_name(&name)
            .and_then(|(task, _)| crate::mail::unread(task, &name).ok())
            .unwrap_or_default();
    }

    fn row_line(&self, row: Row) -> Line<'_> {
        let group = &self.groups[row.group];
        let indent = "  ".repeat(row.depth);
        let mut spans = vec![Span::raw(indent)];
        match row.member {
            None => {
                let subject = optional(group, "subject");
                let arrow = if single(group) {
                    "  "
                } else if self.collapsed.contains(subject) {
                    "▸ "
                } else {
                    "▾ "
                };
                spans.push(Span::styled(arrow, Style::new().fg(MUTED)));
                spans.push(Span::styled(subject, Style::new().bold()));
                if is_director(group) {
                    spans.push(Span::styled("  director", Style::new().fg(ACCENT)));
                    if let Some(lead) = members(group).first() {
                        let (dot, color) = status_style(optional(lead, "status"));
                        spans.push(Span::styled(format!(" {dot}"), Style::new().fg(color)));
                    }
                } else if single(group) {
                    let agent = &members(group)[0];
                    let (dot, color) = status_style(optional(agent, "status"));
                    spans.push(Span::styled(
                        format!("  {} ", optional(agent, "agent")),
                        Style::new().fg(MUTED),
                    ));
                    spans.push(Span::styled(dot, Style::new().fg(color)));
                    return Line::from(spans);
                } else if optional(group, "kind") != "herd" {
                    spans.push(Span::styled("  session", Style::new().fg(MUTED)));
                }
                for (key, color) in [("working", Color::Yellow), ("blocked", Color::Red)] {
                    let count = group[key].as_u64().unwrap_or(0);
                    if count > 0 && !is_director(group) {
                        spans.push(Span::styled(
                            format!("  {count} {key}"),
                            Style::new().fg(color),
                        ));
                    }
                }
            }
            Some(index) => {
                let member = &members(group)[index];
                let status = optional(member, "status");
                let (dot, color) = status_style(status);
                spans.push(Span::raw(format!("{:<7}", optional(member, "role"))));
                spans.push(Span::styled(
                    format!("{:<7}", optional(member, "agent")),
                    Style::new().fg(MUTED),
                ));
                spans.push(Span::styled(
                    format!("{dot} {status}"),
                    Style::new().fg(color),
                ));
            }
        }
        Line::from(spans)
    }

    pub fn draw(&mut self, frame: &mut Frame) {
        let area = frame.area();
        if area.width < MIN_WIDTH || area.height < MIN_HEIGHT {
            frame.render_widget(
                Paragraph::new("Resize the terminal to at least 60 columns and 14 rows. q quits."),
                area,
            );
            return;
        }
        let [header, body, footer] = Layout::vertical([
            Constraint::Length(1),
            Constraint::Min(5),
            Constraint::Length(1),
        ])
        .spacing(1)
        .margin(1)
        .areas(area);
        self.draw_header(frame, header);
        self.draw_footer(frame, footer);
        if self.rows.is_empty() {
            let [middle] = Layout::vertical([Constraint::Length(3)])
                .flex(Flex::Center)
                .areas(body);
            frame.render_widget(
                Paragraph::new(vec![
                    Line::from("No teams run on this herdr server.").fg(MUTED),
                    Line::from(""),
                    Line::from(vec![
                        Span::raw("Press "),
                        Span::styled("n", Style::new().fg(ACCENT).bold()),
                        Span::raw(" to start a team."),
                    ]),
                ])
                .centered(),
                middle,
            );
            return;
        }
        let [tree, detail] =
            Layout::horizontal([Constraint::Percentage(40), Constraint::Percentage(60)])
                .spacing(1)
                .areas(body);
        let items: Vec<ListItem> = self
            .rows
            .iter()
            .map(|&row| ListItem::new(self.row_line(row)))
            .collect();
        let list = List::new(items)
            .block(panel("teams", true))
            .highlight_style(Style::new().bg(Color::Indexed(237)))
            .highlight_symbol("▌");
        let mut state = self.list;
        frame.render_stateful_widget(list, tree, &mut state);
        self.list = state;
        self.draw_detail(frame, detail);
    }

    fn draw_header(&self, frame: &mut Frame, area: Rect) {
        let teams = self
            .groups
            .iter()
            .filter(|g| optional(g, "kind") == "herd" && !is_director(g))
            .count();
        let total = |key: &str| {
            self.groups
                .iter()
                .map(|g| g[key].as_u64().unwrap_or(0))
                .sum::<u64>()
        };
        let mut summary = vec![Span::styled(
            format!("{teams} teams"),
            Style::new().fg(MUTED),
        )];
        for (key, color) in [("working", Color::Yellow), ("blocked", Color::Red)] {
            let count = total(key);
            if count > 0 {
                summary.push(Span::styled(" · ", Style::new().fg(MUTED)));
                summary.push(Span::styled(
                    format!("{count} {key}"),
                    Style::new().fg(color),
                ));
            }
        }
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                "herdr-shepherd",
                Style::new().fg(ACCENT).bold(),
            ))),
            area,
        );
        frame.render_widget(Paragraph::new(Line::from(summary)).right_aligned(), area);
    }

    fn draw_detail(&self, frame: &mut Frame, area: Rect) {
        let Some(agent) = self.agent() else {
            return;
        };
        let inbox_height = (self.inbox.len() as u16 + 2).clamp(3, 8);
        let [pane, inbox] =
            Layout::vertical([Constraint::Min(3), Constraint::Length(inbox_height)]).areas(area);
        let (dot, color) = status_style(optional(agent, "status"));
        let title = Line::from(vec![
            Span::styled(
                format!(" {} ", optional(agent, "name")),
                Style::new().bold(),
            ),
            Span::styled(
                format!("{} · ", optional(agent, "agent")),
                Style::new().fg(MUTED),
            ),
            Span::styled(
                format!("{dot} {} ", optional(agent, "status")),
                Style::new().fg(color),
            ),
        ]);
        let block = panel("", false).title(title).title_bottom(
            Line::from(format!(" {} ", optional(agent, "pane_id")))
                .fg(MUTED)
                .right_aligned(),
        );
        let height = block.inner(pane).height as usize;
        let lines: Vec<&str> = self.pane.trim_end().lines().collect();
        let start = lines.len().saturating_sub(height);
        let text: Vec<Line> = lines[start..].iter().map(|line| Line::raw(*line)).collect();
        frame.render_widget(Paragraph::new(text).block(block), pane);
        let title = if self.inbox.is_empty() {
            " inbox ".to_owned()
        } else {
            format!(" inbox · {} unread ", self.inbox.len())
        };
        let lines: Vec<Line> = if self.inbox.is_empty() {
            vec![Line::from("No unread messages.").fg(MUTED)]
        } else {
            self.inbox
                .iter()
                .map(|(_, from, first)| {
                    Line::from(vec![
                        Span::styled(format!("{from}  "), Style::new().fg(ACCENT)),
                        Span::raw(first.as_str()),
                    ])
                })
                .collect()
        };
        frame.render_widget(Paragraph::new(lines).block(panel(&title, false)), inbox);
    }

    fn draw_footer(&self, frame: &mut Frame, area: Rect) {
        if !self.message.is_empty() {
            frame.render_widget(Paragraph::new(self.message.as_str()).fg(Color::Red), area);
            return;
        }
        let mut spans = Vec::new();
        for (key, label) in [
            ("↑↓", "move"),
            ("←→", "fold"),
            ("⏎", "open tab"),
            ("n", "new team"),
            ("d", "new director"),
            ("q", "quit"),
        ] {
            spans.push(Span::styled(key, Style::new().fg(ACCENT).bold()));
            spans.push(Span::styled(format!(" {label}   "), Style::new().fg(MUTED)));
        }
        frame.render_widget(Paragraph::new(Line::from(spans)), area);
    }
}

fn panel(title: &str, focused: bool) -> Block<'_> {
    let color = if focused { ACCENT } else { MUTED };
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(color))
        .padding(Padding::horizontal(1));
    if title.is_empty() {
        block
    } else {
        block.title(Span::styled(
            format!(" {} ", title.trim()),
            Style::new().fg(color).add_modifier(Modifier::BOLD),
        ))
    }
}

pub fn run(message: String) -> Result<Action> {
    let mut board = Board {
        message,
        ..Board::default()
    };
    board.refresh();
    ratatui::run(|terminal| -> Result<Action> {
        let _paste = PasteMode::on()?;
        let mut last = Instant::now();
        loop {
            if board.stale {
                board.detail();
            }
            terminal.draw(|frame| {
                board.draw(frame);
                if let Some(launcher) = &board.launcher {
                    launcher.draw(frame);
                }
            })?;
            if event::poll(Duration::from_millis(250))? {
                match event::read()? {
                    Event::Key(key) if key.kind == KeyEventKind::Press => match board.key(key) {
                        Ok(Some(action)) => return Ok(action),
                        Ok(None) => {}
                        Err(error) => board.message = error.to_string(),
                    },
                    Event::Paste(text) => {
                        if let Some(launcher) = &mut board.launcher {
                            launcher.insert(&text);
                        }
                    }
                    _ => {}
                }
            }
            if last.elapsed() >= REFRESH {
                board.refresh();
                last = Instant::now();
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{Terminal, backend::TestBackend};
    use serde_json::json;

    fn member(name: &str, role: &str, status: &str) -> Value {
        json!({"name": name, "role": role, "agent": "claude", "status": status,
            "pane_id": format!("p-{name}"), "tab_id": "t1", "cwd": "/x"})
    }

    fn sample() -> Vec<Value> {
        vec![
            json!({"subject": "org", "kind": "herd", "under": "", "working": 0, "blocked": 0,
                "members": [member("org-director", "director", "idle")]}),
            json!({"subject": "auth", "kind": "herd", "under": "org", "working": 1, "blocked": 0,
                "members": [member("auth-boss", "boss", "working"), member("auth-w1", "w1", "idle")]}),
            json!({"subject": "docs", "kind": "herd", "under": "", "working": 0, "blocked": 1,
                "members": [member("docs-boss", "boss", "blocked")]}),
        ]
    }

    fn press(board: &mut Board, code: KeyCode) -> Option<Action> {
        board.key(KeyEvent::new(code, KeyModifiers::NONE)).unwrap()
    }

    fn keys(board: &Board) -> Vec<String> {
        board.rows.iter().map(|&row| board.key_of(row)).collect()
    }

    fn screen(board: &mut Board, width: u16, height: u16) -> String {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|frame| board.draw(frame)).unwrap();
        terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect()
    }

    #[test]
    fn teams_nest_under_their_director() {
        let mut board = Board::default();
        board.load(sample());
        assert_eq!(
            keys(&board),
            [
                "org",
                "auth",
                "auth/auth-boss",
                "auth/auth-w1",
                "docs",
                "docs/docs-boss"
            ]
        );
        assert_eq!(board.rows[1].depth, 1);
        assert_eq!(optional(board.agent().unwrap(), "name"), "org-director");
        press(&mut board, KeyCode::Down);
        assert_eq!(optional(board.agent().unwrap(), "name"), "auth-boss");
    }

    #[test]
    fn a_single_session_uses_one_row() {
        let mut board = Board::default();
        let mut groups = sample();
        groups.push(json!({"subject": "notes", "kind": "session", "under": "",
            "members": [member("", "claude", "idle")]}));
        board.load(groups);
        assert_eq!(keys(&board).last().unwrap(), "notes");
        board.list.select(Some(board.rows.len() - 1));
        assert_eq!(optional(board.agent().unwrap(), "pane_id"), "p-");
    }

    #[test]
    fn a_missing_director_leaves_its_team_at_the_top() {
        let mut board = Board::default();
        board.load(sample().split_off(1));
        assert_eq!(board.rows[0].depth, 0);
        assert_eq!(keys(&board)[0], "auth");
    }

    #[test]
    fn folding_and_reloading_keep_the_selection() {
        let mut board = Board::default();
        board.load(sample());
        press(&mut board, KeyCode::Down);
        press(&mut board, KeyCode::Down);
        press(&mut board, KeyCode::Left);
        assert_eq!(board.key_of(board.selected_row().unwrap()), "auth");
        press(&mut board, KeyCode::Left);
        assert_eq!(keys(&board), ["org", "auth", "docs", "docs/docs-boss"]);
        board.load(sample());
        assert_eq!(board.key_of(board.selected_row().unwrap()), "auth");
        press(&mut board, KeyCode::Right);
        assert_eq!(keys(&board).len(), 6);
        assert!(press(&mut board, KeyCode::Char('n')).is_none());
        assert_eq!(board.selected_director().as_deref(), Some("org"));
        assert!(board.launcher.is_some());
        assert!(press(&mut board, KeyCode::Char('q')).is_none());
        assert!(press(&mut board, KeyCode::Esc).is_none());
        assert!(board.launcher.is_none());
        assert!(matches!(
            press(&mut board, KeyCode::Char('q')),
            Some(Action::Quit)
        ));
    }

    #[test]
    fn board_renders_the_tree_detail_and_empty_state() {
        let mut board = Board::default();
        board.load(sample());
        board.pane = "first\nlast line".into();
        board.inbox = vec![("m1".into(), "org-director".into(), "Use the cache.".into())];
        let text = screen(&mut board, 110, 24);
        for expected in [
            "herdr-shepherd",
            "1 working",
            "1 blocked",
            "director",
            "org-director",
            "last line",
            "1 unread",
            "Use the cache.",
            "new team",
        ] {
            assert!(text.contains(expected), "{expected}");
        }
        let mut empty = Board::default();
        empty.load(Vec::new());
        assert!(screen(&mut empty, 80, 20).contains("No teams run"));
        assert!(screen(&mut empty, 40, 10).contains("Resize"));
    }
}
