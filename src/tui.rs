use crate::{Result, cli, interactive, process};
use crossterm::{
    event::{
        self, DisableBracketedPaste, EnableBracketedPaste, Event, KeyCode, KeyEvent, KeyEventKind,
        KeyModifiers,
    },
    execute,
};
use ratatui::{
    Frame,
    layout::{Constraint, Layout},
    style::{Color, Style, Stylize},
    text::Line,
    widgets::{Block, BorderType, Paragraph, Wrap},
};
use std::{env, io};

const LABELS: [&str; 5] = [
    "Task name",
    "Working folder",
    "Boss agent",
    "Worker agent",
    "Worker count",
];
const MIN_WIDTH: u16 = 64;
const MIN_HEIGHT: u16 = 24;

struct Form {
    values: [String; 5],
    fresh: [bool; 5],
    focus: usize,
    review: bool,
    error: String,
    dry: bool,
}

impl Form {
    fn new(dry: bool) -> Result<Self> {
        let cwd = env::current_dir()?;
        Ok(Self {
            values: [
                "my-task".into(),
                cwd.to_str()
                    .ok_or("The folder path must use UTF-8.")?
                    .into(),
                "claude".into(),
                "claude".into(),
                "3".into(),
            ],
            fresh: [true; 5],
            focus: 0,
            review: false,
            error: String::new(),
            dry,
        })
    }

    fn args(&self) -> Vec<String> {
        let mut args = vec![
            self.values[0].clone(),
            "--cwd".into(),
            self.values[1].clone(),
            "--boss-agent".into(),
            self.values[2].clone(),
        ];
        args.extend(interactive::worker_args(&self.values[3]));
        args.extend(["--workers".into(), self.values[4].clone()]);
        if self.dry {
            args.push("--dry-run".into());
        }
        args
    }

    fn command(&self) -> String {
        let mut command = vec!["herdr-shepherd".into()];
        command.extend(self.args());
        process::shell_command(&command)
    }

    fn validate(&mut self, index: usize) -> Result<()> {
        let value = self.values[index].trim();
        self.values[index] = match index {
            0 => {
                cli::task_name(value)?;
                value.to_owned()
            }
            1 => interactive::folder(value)?,
            2 => interactive::selected_agent(value)?,
            3 => {
                let entries = interactive::selected_agents(value)?;
                // A list sets the count unless the user typed a count.
                if entries.len() > 1 && self.fresh[4] {
                    self.values[4] = entries.len().to_string();
                }
                entries.join(",")
            }
            _ => cli::worker_count(value)?.to_string(),
        };
        Ok(())
    }

    fn options(&mut self) -> Result<cli::Options> {
        for index in 0..5 {
            if let Err(error) = self.validate(index) {
                self.focus = index;
                return Err(error);
            }
        }
        cli::parse(&self.args())?.ok_or_else(|| "The team options are missing.".into())
    }

    fn insert(&mut self, text: &str) {
        let text: String = text.chars().filter(|c| !c.is_control()).collect();
        if text.is_empty() {
            return;
        }
        if self.fresh[self.focus] {
            self.values[self.focus].clear();
        }
        self.fresh[self.focus] = false;
        self.values[self.focus].push_str(&text);
        self.error.clear();
    }

    fn key(&mut self, key: KeyEvent) -> Result<Option<cli::Options>> {
        if self.review {
            match key.code {
                KeyCode::Enter => return self.options().map(Some),
                KeyCode::Backspace | KeyCode::BackTab => self.review = false,
                _ => {}
            }
            return Ok(None);
        }
        self.error.clear();
        match key.code {
            KeyCode::Tab | KeyCode::Down => self.focus = (self.focus + 1) % 5,
            KeyCode::BackTab | KeyCode::Up => self.focus = (self.focus + 4) % 5,
            KeyCode::Enter => {
                self.validate(self.focus)?;
                if self.focus < 4 {
                    self.focus += 1;
                } else {
                    self.options()?;
                    self.review = true;
                }
            }
            KeyCode::Left | KeyCode::Right
                if matches!(self.focus, 2 | 3)
                    && matches!(self.values[self.focus].as_str(), "claude" | "codex") =>
            {
                self.values[self.focus] = if self.values[self.focus] == "claude" {
                    "codex"
                } else {
                    "claude"
                }
                .into();
                self.fresh[self.focus] = false;
            }
            KeyCode::Backspace => {
                self.values[self.focus].pop();
                self.fresh[self.focus] = false;
            }
            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.values[self.focus].clear();
                self.fresh[self.focus] = false;
            }
            KeyCode::Char(c)
                if !key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                self.insert(&c.to_string())
            }
            _ => {}
        }
        Ok(None)
    }

    fn draw(&self, frame: &mut Frame) {
        let area = frame.area();
        if area.width < MIN_WIDTH || area.height < MIN_HEIGHT {
            frame.render_widget(
                Paragraph::new(
                    "Resize the terminal to at least 64 columns and 24 rows. Esc cancels.",
                )
                .wrap(Wrap { trim: true }),
                area,
            );
            return;
        }
        let [header, body, footer] = Layout::vertical([
            Constraint::Length(3),
            Constraint::Min(15),
            Constraint::Length(4),
        ])
        .margin(1)
        .areas(area);
        frame.render_widget(
            Paragraph::new(vec![
                Line::from("herdr-shepherd").cyan().bold(),
                Line::from("Set up a Claude Code and Codex team."),
            ]),
            header,
        );
        let [fields, summary] =
            Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)])
                .spacing(2)
                .areas(body);
        let rows = Layout::vertical([Constraint::Length(3); 5]).split(fields);
        for index in 0..5 {
            let active = self.focus == index && !self.review;
            let title = format!("{}{}", if active { "> " } else { "" }, LABELS[index]);
            let color = if active { Color::Cyan } else { Color::DarkGray };
            let block = Block::bordered()
                .border_type(BorderType::Rounded)
                .title(title)
                .border_style(Style::default().fg(color));
            let inner = block.inner(rows[index]);
            let line = Line::from(self.values[index].as_str());
            let scroll = line
                .width()
                .saturating_sub(inner.width.saturating_sub(1) as usize)
                .min(u16::MAX as usize) as u16;
            frame.render_widget(
                Paragraph::new(line).block(block).scroll((0, scroll)),
                rows[index],
            );
            if active && inner.width > 0 {
                let x = Line::from(self.values[index].as_str())
                    .width()
                    .saturating_sub(scroll as usize)
                    .min(inner.width.saturating_sub(1) as usize) as u16;
                frame.set_cursor_position((inner.x + x, inner.y));
            }
        }
        let title = if self.review {
            "Review team"
        } else {
            "Command preview"
        };
        let action = if self.dry {
            "Enter previews the commands. No panes are created."
        } else {
            "Enter starts this team after review."
        };
        let description = format!(
            "{}\n\n{}\n\nAgents: claude or 1; codex or 2. Add :model to select a model.\nLeft/Right switches between claude and codex.\n\nSeparate worker entries with commas. A list of two or more sets the worker count.\nThe working folder accepts ~ and relative paths.",
            self.command(),
            action
        );
        frame.render_widget(
            Paragraph::new(description)
                .wrap(Wrap { trim: false })
                .block(
                    Block::bordered()
                        .border_type(BorderType::Rounded)
                        .title(title),
                ),
            summary,
        );
        let help = if self.review {
            "Enter: confirm | Backspace: edit | Esc/Ctrl+C: cancel"
        } else {
            "Tab/Up/Down: field | Enter: next/review | Type: replace default\nBackspace: delete | Ctrl+U: clear | Esc/Ctrl+C: cancel"
        };
        let message = if self.error.is_empty() {
            "No team is created until you confirm."
        } else {
            &self.error
        };
        let mut lines: Vec<Line> = help.lines().map(Line::from).collect();
        lines.push(Line::from(message).fg(if self.error.is_empty() {
            Color::DarkGray
        } else {
            Color::Red
        }));
        frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: true }), footer);
    }
}

struct PasteMode;
impl Drop for PasteMode {
    fn drop(&mut self) {
        let _ = execute!(io::stdout(), DisableBracketedPaste);
    }
}

pub fn setup(dry: bool) -> Result<Option<cli::Options>> {
    let mut form = Form::new(dry)?;
    let options = ratatui::run(|terminal| -> Result<Option<cli::Options>> {
        execute!(io::stdout(), EnableBracketedPaste)?;
        let _paste_mode = PasteMode;
        loop {
            terminal.draw(|frame| form.draw(frame))?;
            match event::read()? {
                Event::Key(key) if key.kind == KeyEventKind::Press => {
                    if key.code == KeyCode::Esc
                        || key.code == KeyCode::Char('c')
                            && key.modifiers.contains(KeyModifiers::CONTROL)
                    {
                        return Ok(None);
                    }
                    let size = terminal.size()?;
                    if size.width < MIN_WIDTH || size.height < MIN_HEIGHT {
                        continue;
                    }
                    match form.key(key) {
                        Ok(Some(options)) => return Ok(Some(options)),
                        Ok(None) => {}
                        Err(error) => {
                            form.review = false;
                            form.error = error.to_string();
                        }
                    }
                }
                Event::Paste(text) if !form.review => form.insert(&text),
                _ => {}
            }
        }
    })?;
    if options.is_none() {
        println!("Setup cancelled. No team was created.");
    }
    Ok(options)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{Terminal, backend::TestBackend};

    fn press(form: &mut Form, code: KeyCode) -> Result<Option<cli::Options>> {
        form.key(KeyEvent::new(code, KeyModifiers::NONE))
    }

    #[test]
    fn enter_requires_review_before_returning_launch_options() {
        let mut form = Form::new(true).unwrap();
        for _ in 0..5 {
            assert!(press(&mut form, KeyCode::Enter).unwrap().is_none());
        }
        assert!(form.review);
        let options = press(&mut form, KeyCode::Enter).unwrap().unwrap();
        assert_eq!(options.worker_agents, vec!["claude"; 3]);
        assert!(options.dry);
    }

    #[test]
    fn invalid_input_returns_to_its_field_and_paste_cannot_submit() {
        let mut form = Form::new(false).unwrap();
        form.insert("bad task\n\r\x1b");
        assert_eq!(form.values[0], "bad task");
        form.focus = 4;
        assert!(press(&mut form, KeyCode::Enter).is_err());
        assert_eq!(form.focus, 0);
        assert!(!form.review);
        form.insert("-h");
        assert!(form.options().is_err());
    }

    #[test]
    fn agents_can_change_and_review_can_return_to_editing() {
        let mut form = Form::new(false).unwrap();
        form.focus = 2;
        press(&mut form, KeyCode::Right).unwrap();
        assert_eq!(form.values[2], "codex");
        form.focus = 4;
        form.insert("0");
        press(&mut form, KeyCode::Enter).unwrap();
        assert!(form.review);
        press(&mut form, KeyCode::Backspace).unwrap();
        assert!(!form.review);
        assert!(form.options().unwrap().worker_agents.is_empty());
    }

    #[test]
    fn worker_list_sets_the_count_unless_the_user_typed_a_count() {
        let mut form = Form::new(true).unwrap();
        form.focus = 3;
        form.insert("codex:m1, 2,claude:opus");
        press(&mut form, KeyCode::Right).unwrap();
        assert_eq!(form.values[3], "codex:m1, 2,claude:opus");
        press(&mut form, KeyCode::Enter).unwrap();
        assert_eq!(form.values[3], "codex:m1,codex,claude:opus");
        assert_eq!(form.values[4], "3");
        form.focus = 3;
        form.values[3] = "codex,claude".into();
        press(&mut form, KeyCode::Enter).unwrap();
        assert_eq!(form.values[4], "2");
        let options = form.options().unwrap();
        assert_eq!(options.worker_agents, ["codex", "claude"]);
        assert_eq!(options.worker_models, ["", ""]);
        form.focus = 4;
        form.insert("3");
        assert!(form.options().is_err());
        form.focus = 3;
        form.insert("codex:");
        assert!(press(&mut form, KeyCode::Enter).is_err());
    }

    #[test]
    fn form_renders_fields_and_small_terminal_instructions() {
        let form = Form::new(false).unwrap();
        for (width, height, expected) in [
            (100, 28, "Command preview"),
            (40, 10, "Resize the terminal"),
        ] {
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            terminal.draw(|frame| form.draw(frame)).unwrap();
            let buffer = terminal.backend().buffer();
            let text: String = buffer.content.iter().map(|cell| cell.symbol()).collect();
            assert!(text.contains(expected));
        }
    }
}
