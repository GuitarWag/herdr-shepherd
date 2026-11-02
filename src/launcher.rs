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
    layout::{Constraint, Flex, Layout},
    style::{Color, Style, Stylize},
    text::{Line, Span},
    widgets::{Block, BorderType, Clear, Padding, Paragraph, Wrap},
};
use std::{env, fs, io, path::Path};

const ACCENT: Color = Color::Cyan;
const MUTED: Color = Color::DarkGray;
// shortcut: the card shows 8 worker rows; use the CLI for larger teams.
const MAX_WORKERS: usize = 8;
const AGENTS: [&str; 2] = ["claude", "codex"];

fn presets(agent: &str) -> &'static [&'static str] {
    if agent == "claude" {
        &["", "fable", "opus", "sonnet", "haiku"]
    } else {
        &["", "gpt-6-astra", "gpt-6.1-sol"]
    }
}

/// An agent type and an optional model. An empty model uses the role default.
#[derive(Clone, PartialEq, Debug)]
struct Pick {
    agent: &'static str,
    model: String,
}

impl Pick {
    fn new(agent: &'static str) -> Self {
        Self {
            agent,
            model: String::new(),
        }
    }

    fn entry(&self) -> String {
        if self.model.is_empty() {
            self.agent.to_owned()
        } else {
            format!("{}:{}", self.agent, self.model)
        }
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Field {
    Kind,
    Name,
    Folder,
    Under,
    LeadAgent,
    LeadModel,
    Count,
    Agent(usize),
    Model(usize),
}

pub enum Outcome {
    Cancel,
    Launch(Box<cli::Options>),
}

pub struct Launcher {
    director: bool,
    name: String,
    folder: String,
    /// The folder still holds the default, so typing replaces it.
    folder_fresh: bool,
    directors: Vec<String>,
    /// The index in `directors` plus one. Zero means no director.
    under: usize,
    lead: Pick,
    workers: Vec<Pick>,
    focus: Field,
    error: String,
    dry: bool,
}

fn home_short(path: &str) -> String {
    match env::var("HOME") {
        Ok(home) if !home.is_empty() && path.starts_with(&home) => {
            format!("~{}", &path[home.len()..])
        }
        _ => path.to_owned(),
    }
}

/// Complete the last part of a folder path to the longest shared prefix.
fn complete(value: &str) -> Option<String> {
    let expanded = match value.strip_prefix('~') {
        Some(rest) => format!("{}{rest}", env::var("HOME").ok()?),
        None => value.to_owned(),
    };
    let (parent, prefix) = match expanded.rfind('/') {
        Some(index) => (&expanded[..=index], &expanded[index + 1..]),
        None => ("./", expanded.as_str()),
    };
    let mut names: Vec<String> = fs::read_dir(Path::new(parent))
        .ok()?
        .flatten()
        .filter(|entry| entry.path().is_dir())
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|name| {
            name.starts_with(prefix) && (prefix.starts_with('.') || !name.starts_with('.'))
        })
        .collect();
    names.sort();
    let first = names.first()?;
    let mut shared = first.clone();
    for name in &names[1..] {
        while !name.starts_with(&shared) {
            shared.pop();
        }
    }
    if names.len() == 1 {
        shared.push('/');
    }
    if shared.len() <= prefix.len() && names.len() > 1 {
        return None;
    }
    let keep = &value[..value.len() - prefix.len()];
    Some(format!("{keep}{shared}"))
}

impl Launcher {
    pub fn new(
        director: bool,
        directors: Vec<String>,
        under: Option<&str>,
        dry: bool,
    ) -> Result<Self> {
        let cwd = env::current_dir()?;
        let cwd = cwd.to_str().ok_or("The folder path must use UTF-8.")?;
        let under = under
            .and_then(|name| directors.iter().position(|d| d == name))
            .map_or(0, |index| index + 1);
        Ok(Self {
            director,
            name: String::new(),
            folder: home_short(cwd),
            folder_fresh: true,
            directors,
            under,
            lead: Pick::new("claude"),
            workers: vec![Pick::new("claude"); 3],
            focus: Field::Name,
            error: String::new(),
            dry,
        })
    }

    fn fields(&self) -> Vec<Field> {
        let mut fields = vec![Field::Kind, Field::Name, Field::Folder];
        if !self.director && !self.directors.is_empty() {
            fields.push(Field::Under);
        }
        fields.extend([Field::LeadAgent, Field::LeadModel]);
        if !self.director {
            fields.push(Field::Count);
            for index in 0..self.workers.len() {
                fields.extend([Field::Agent(index), Field::Model(index)]);
            }
        }
        fields
    }

    fn step(&mut self, forward: bool) {
        let fields = self.fields();
        let index = fields.iter().position(|f| *f == self.focus).unwrap_or(0);
        let next = if forward {
            (index + 1) % fields.len()
        } else {
            (index + fields.len() - 1) % fields.len()
        };
        self.focus = fields[next];
    }

    fn pick(&mut self, field: Field) -> Option<&mut Pick> {
        match field {
            Field::LeadAgent | Field::LeadModel => Some(&mut self.lead),
            Field::Agent(index) | Field::Model(index) => self.workers.get_mut(index),
            _ => None,
        }
    }

    fn change(&mut self, forward: bool) {
        match self.focus {
            Field::Kind => {
                self.director = !self.director;
            }
            Field::Under => {
                let count = self.directors.len() + 1;
                self.under = if forward {
                    (self.under + 1) % count
                } else {
                    (self.under + count - 1) % count
                };
            }
            Field::Count => {
                let count = if forward {
                    (self.workers.len() + 1).min(MAX_WORKERS)
                } else {
                    self.workers.len().saturating_sub(1)
                };
                let last = self.workers.last().cloned().unwrap_or(Pick::new("claude"));
                self.workers.resize(count, last);
            }
            field @ (Field::LeadAgent | Field::Agent(_)) => {
                if let Some(pick) = self.pick(field) {
                    pick.agent = if pick.agent == AGENTS[0] {
                        AGENTS[1]
                    } else {
                        AGENTS[0]
                    };
                    pick.model.clear();
                }
            }
            field @ (Field::LeadModel | Field::Model(_)) => {
                if let Some(pick) = self.pick(field) {
                    let list = presets(pick.agent);
                    let next = match list.iter().position(|m| *m == pick.model) {
                        Some(index) if forward => (index + 1) % list.len(),
                        Some(index) => (index + list.len() - 1) % list.len(),
                        None => 0,
                    };
                    pick.model = list[next].to_owned();
                }
            }
            Field::Name | Field::Folder => {}
        }
    }

    fn text(&mut self) -> Option<&mut String> {
        match self.focus {
            Field::Name => Some(&mut self.name),
            Field::Folder => Some(&mut self.folder),
            field @ (Field::LeadModel | Field::Model(_)) => self.pick(field).map(|p| &mut p.model),
            _ => None,
        }
    }

    pub fn insert(&mut self, text: &str) {
        let text: String = text.chars().filter(|c| !c.is_control()).collect();
        if text.is_empty() {
            return;
        }
        let focus = self.focus;
        let fresh = self.folder_fresh && focus == Field::Folder;
        let preset = self
            .pick(focus)
            .is_some_and(|pick| presets(pick.agent).contains(&pick.model.as_str()));
        if let Some(value) = self.text() {
            if fresh || (preset && matches!(focus, Field::LeadModel | Field::Model(_))) {
                value.clear();
            }
            value.push_str(&text);
        }
        if focus == Field::Folder {
            self.folder_fresh = false;
        }
    }

    fn args(&self) -> Vec<String> {
        let folder = self.folder.trim();
        // The preview must work in a shell, which does not expand a quoted ~.
        let folder = interactive::folder(folder).unwrap_or_else(|_| folder.to_owned());
        let mut args = vec![self.name.trim().to_owned(), "--cwd".into(), folder];
        if self.director {
            args.push("--director".into());
        } else if self.under > 0 {
            args.extend(["--under".into(), self.directors[self.under - 1].clone()]);
        }
        args.extend(["--boss-agent".into(), self.lead.entry()]);
        if !self.director {
            args.extend(["--workers".into(), self.workers.len().to_string()]);
            for worker in &self.workers {
                args.extend(["--worker-agent".into(), worker.entry()]);
            }
        }
        if self.dry {
            args.push("--dry-run".into());
        }
        args
    }

    fn options(&mut self) -> Result<cli::Options> {
        if self.name.trim().is_empty() {
            self.focus = Field::Name;
            return Err("Enter a task name.".into());
        }
        if let Err(error) = cli::task_name(self.name.trim()) {
            self.focus = Field::Name;
            return Err(error);
        }
        if let Err(error) = interactive::folder(self.folder.trim()) {
            self.focus = Field::Folder;
            return Err(error);
        }
        cli::parse(&self.args())?.ok_or_else(|| "The team options are missing.".into())
    }

    pub fn key(&mut self, key: KeyEvent) -> Option<Outcome> {
        self.error.clear();
        let control = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Esc => return Some(Outcome::Cancel),
            KeyCode::Char('c') if control => return Some(Outcome::Cancel),
            KeyCode::Enter => match self.options() {
                Ok(options) => return Some(Outcome::Launch(Box::new(options))),
                Err(error) => self.error = error.to_string(),
            },
            KeyCode::Tab if self.focus == Field::Folder => match complete(&self.folder) {
                Some(value) => {
                    self.folder = value;
                    self.folder_fresh = false;
                }
                None => self.step(true),
            },
            KeyCode::Down | KeyCode::Tab => self.step(true),
            KeyCode::Up | KeyCode::BackTab => self.step(false),
            KeyCode::Left => self.change(false),
            KeyCode::Right => self.change(true),
            KeyCode::Char('-') if self.focus == Field::Count => self.change(false),
            KeyCode::Char('+') if self.focus == Field::Count => self.change(true),
            KeyCode::Char(' ') if self.text().is_none() => self.change(true),
            KeyCode::Backspace => {
                if self.focus == Field::Folder {
                    self.folder_fresh = false;
                }
                if let Some(value) = self.text() {
                    value.pop();
                }
            }
            KeyCode::Char('u') if control => {
                if let Some(value) = self.text() {
                    value.clear();
                }
            }
            KeyCode::Char(c) if !control && !key.modifiers.contains(KeyModifiers::ALT) => {
                self.insert(&c.to_string());
            }
            _ => {}
        }
        None
    }

    fn label(&self, text: &str, fields: &[Field]) -> Span<'static> {
        let focused = fields.contains(&self.focus);
        let marker = if focused { "› " } else { "  " };
        let style = if focused {
            Style::new().fg(ACCENT).bold()
        } else {
            Style::new().fg(MUTED)
        };
        Span::styled(format!("{marker}{text:<9}"), style)
    }

    fn choice(&self, options: &[&str], selected: usize, field: Field) -> Vec<Span<'static>> {
        let focused = self.focus == field;
        options
            .iter()
            .enumerate()
            .map(|(index, option)| {
                let on = index == selected;
                let style = match (on, focused) {
                    (true, true) => Style::new().fg(ACCENT).bold(),
                    (true, false) => Style::new().bold(),
                    _ => Style::new().fg(MUTED),
                };
                Span::styled(format!("{} {option}   ", if on { "◉" } else { "○" }), style)
            })
            .collect()
    }

    fn stepper(&self, value: String, dim: bool, field: Field) -> Vec<Span<'static>> {
        let focused = self.focus == field;
        let arrows = Style::new().fg(if focused { ACCENT } else { MUTED });
        let style = match (focused, dim) {
            (_, true) => Style::new().fg(MUTED),
            (true, false) => Style::new().fg(ACCENT).bold(),
            _ => Style::new(),
        };
        vec![
            Span::styled("◂ ", arrows),
            Span::styled(value, style),
            Span::styled(" ▸", arrows),
        ]
    }

    fn agent_row(&self, label: &str, pick: &Pick, lead: bool, fields: [Field; 2]) -> Line<'static> {
        let selected = AGENTS.iter().position(|a| *a == pick.agent).unwrap_or(0);
        let mut spans = vec![self.label(label, &fields)];
        spans.extend(self.choice(&AGENTS, selected, fields[0]));
        spans.push(Span::styled("model ", Style::new().fg(MUTED)));
        let (text, dim) = if pick.model.is_empty() {
            (cli::default_model(pick.agent, lead).to_owned(), true)
        } else {
            (pick.model.clone(), false)
        };
        spans.extend(self.stepper(text, dim, fields[1]));
        Line::from(spans)
    }

    /// The card lines and, for a text field, the line and column of the cursor.
    fn lines(&self) -> (Vec<Line<'static>>, Option<(usize, usize)>) {
        let mut lines = Vec::new();
        let mut cursor = None;
        let kind = [Field::Kind];
        let mut row = vec![self.label("kind", &kind)];
        row.extend(self.choice(
            &["team", "director"],
            usize::from(self.director),
            Field::Kind,
        ));
        lines.push(Line::from(row));
        let text_row = |launcher: &Self, label: &str, field: Field, value: &str, hint: &str| {
            let mut spans = vec![launcher.label(label, &[field])];
            if value.is_empty() {
                spans.push(Span::styled(
                    hint.to_owned(),
                    Style::new().fg(MUTED).italic(),
                ));
            } else {
                spans.push(Span::raw(value.to_owned()));
            }
            Line::from(spans)
        };
        lines.push(text_row(
            self,
            "name",
            Field::Name,
            &self.name,
            "task name, for example auth",
        ));
        if self.focus == Field::Name {
            cursor = Some((lines.len() - 1, 11 + self.name.chars().count()));
        }
        lines.push(text_row(self, "folder", Field::Folder, &self.folder, ""));
        if self.focus == Field::Folder {
            cursor = Some((lines.len() - 1, 11 + self.folder.chars().count()));
        }
        if !self.director && !self.directors.is_empty() {
            let (value, dim) = match self.under {
                0 => ("none".to_owned(), true),
                index => (format!("{}-director", self.directors[index - 1]), false),
            };
            let mut spans = vec![self.label("under", &[Field::Under])];
            spans.extend(self.stepper(value, dim, Field::Under));
            lines.push(Line::from(spans));
        }
        let lead = if self.director { "director" } else { "boss" };
        lines.push(self.agent_row(lead, &self.lead, true, [Field::LeadAgent, Field::LeadModel]));
        if !self.director {
            let mut spans = vec![self.label("workers", &[Field::Count])];
            spans.extend(self.stepper(self.workers.len().to_string(), false, Field::Count));
            lines.push(Line::from(spans));
            for (index, worker) in self.workers.iter().enumerate() {
                let line = self.agent_row(
                    &format!("  w{}", index + 1),
                    worker,
                    false,
                    [Field::Agent(index), Field::Model(index)],
                );
                lines.push(line);
            }
        }
        (lines, cursor)
    }

    pub fn draw(&self, frame: &mut Frame) {
        let area = frame.area();
        let (lines, cursor) = self.lines();
        let mut command = vec!["herdr-shepherd".to_owned()];
        command.extend(self.args());
        let command = process::shell_command(&command);
        let width = area.width.saturating_sub(4).min(80);
        let inner = width.saturating_sub(4).max(1) as usize;
        let preview = command.chars().count().div_ceil(inner) as u16;
        let height = lines.len() as u16 + preview + 6;
        if area.width < 50 || area.height < height.min(16) {
            frame.render_widget(Clear, area);
            frame.render_widget(
                Paragraph::new("Resize the terminal to show the launcher. Esc cancels.")
                    .wrap(Wrap { trim: true }),
                area,
            );
            return;
        }
        let [card] = Layout::horizontal([Constraint::Length(width)])
            .flex(Flex::Center)
            .areas(area);
        let [card] = Layout::vertical([Constraint::Length(height.min(area.height))])
            .flex(Flex::Center)
            .areas(card);
        let title = match (self.director, self.dry) {
            (true, false) => " new director ",
            (true, true) => " new director · dry run ",
            (false, false) => " new team ",
            (false, true) => " new team · dry run ",
        };
        let action = if self.dry { "preview" } else { "launch" };
        let mut hints = Vec::new();
        for (key, label) in [
            ("↑↓", "field"),
            ("←→", "change"),
            ("⏎", action),
            ("esc", "cancel"),
        ] {
            hints.push(Span::styled(
                format!(" {key}"),
                Style::new().fg(ACCENT).bold(),
            ));
            hints.push(Span::styled(format!(" {label} "), Style::new().fg(MUTED)));
        }
        let block = Block::bordered()
            .border_type(BorderType::Rounded)
            .border_style(Style::new().fg(ACCENT))
            .title(Span::styled(title, Style::new().fg(ACCENT).bold()))
            .title_bottom(Line::from(hints))
            .padding(Padding::new(1, 1, 1, 0));
        let body = block.inner(card);
        frame.render_widget(Clear, card);
        frame.render_widget(block, card);
        let [fields, _, preview_area, error] = Layout::vertical([
            Constraint::Length(lines.len() as u16),
            Constraint::Length(1),
            Constraint::Length(preview),
            Constraint::Length(1),
        ])
        .areas(body);
        frame.render_widget(Paragraph::new(lines), fields);
        frame.render_widget(
            Paragraph::new(command).fg(MUTED).wrap(Wrap { trim: false }),
            preview_area,
        );
        frame.render_widget(Paragraph::new(self.error.as_str()).fg(Color::Red), error);
        if let Some((line, column)) = cursor {
            let x = fields.x + (column as u16).min(fields.width.saturating_sub(1));
            frame.set_cursor_position((x, fields.y + line as u16));
        }
    }
}

pub struct PasteMode;
impl PasteMode {
    pub fn on() -> Result<Self> {
        execute!(io::stdout(), EnableBracketedPaste)?;
        Ok(Self)
    }
}
impl Drop for PasteMode {
    fn drop(&mut self) {
        let _ = execute!(io::stdout(), DisableBracketedPaste);
    }
}

/// Run the launcher alone, for `--interactive`.
pub fn setup(dry: bool) -> Result<Option<cli::Options>> {
    let directors = crate::view::groups(None)
        .map(|groups| crate::board::directors(&groups))
        .unwrap_or_default();
    let mut launcher = Launcher::new(false, directors, None, dry)?;
    let options = ratatui::run(|terminal| -> Result<Option<cli::Options>> {
        let _paste = PasteMode::on()?;
        loop {
            terminal.draw(|frame| launcher.draw(frame))?;
            match event::read()? {
                Event::Key(key) if key.kind == KeyEventKind::Press => match launcher.key(key) {
                    Some(Outcome::Cancel) => return Ok(None),
                    Some(Outcome::Launch(options)) => return Ok(Some(*options)),
                    None => {}
                },
                Event::Paste(text) => launcher.insert(&text),
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

    fn launcher() -> Launcher {
        Launcher::new(false, vec!["org".into()], None, true).unwrap()
    }

    fn press(launcher: &mut Launcher, code: KeyCode) -> Option<Outcome> {
        launcher.key(KeyEvent::new(code, KeyModifiers::NONE))
    }

    fn launch(launcher: &mut Launcher) -> cli::Options {
        match press(launcher, KeyCode::Enter) {
            Some(Outcome::Launch(options)) => *options,
            _ => panic!("{}", launcher.error),
        }
    }

    #[test]
    fn enter_requires_a_name_then_launches_the_defaults() {
        let mut launcher = launcher();
        assert!(press(&mut launcher, KeyCode::Enter).is_none());
        assert_eq!(launcher.error, "Enter a task name.");
        launcher.insert("auth");
        let options = launch(&mut launcher);
        assert_eq!(options.task, "auth");
        assert_eq!(options.worker_agents, ["claude"; 3]);
        assert!(options.under.is_empty());
        assert!(options.dry);
    }

    #[test]
    fn choices_change_agents_models_workers_and_director() {
        let mut launcher = Launcher::new(false, vec!["org".into()], Some("org"), true).unwrap();
        launcher.insert("auth");
        launcher.focus = Field::LeadAgent;
        press(&mut launcher, KeyCode::Right);
        press(&mut launcher, KeyCode::Down);
        press(&mut launcher, KeyCode::Right);
        assert_eq!(launcher.lead.entry(), "codex:gpt-6-astra");
        launcher.focus = Field::Count;
        press(&mut launcher, KeyCode::Left);
        launcher.focus = Field::Agent(1);
        press(&mut launcher, KeyCode::Right);
        launcher.focus = Field::Model(0);
        launcher.insert("my-model");
        let options = launch(&mut launcher);
        assert_eq!(options.under, "org");
        assert_eq!(options.boss_agent, "codex");
        assert_eq!(options.boss_model, "gpt-6-astra");
        assert_eq!(options.worker_agents, ["claude", "codex"]);
        assert_eq!(options.worker_models, ["my-model", ""]);
        launcher.focus = Field::Kind;
        press(&mut launcher, KeyCode::Right);
        assert!(!launcher.fields().contains(&Field::Count));
        let options = launch(&mut launcher);
        assert!(options.director);
        assert!(options.worker_agents.is_empty());
    }

    #[test]
    fn invalid_values_return_focus_to_their_field() {
        let mut launcher = launcher();
        launcher.insert("bad name\n\x1b");
        assert_eq!(launcher.name, "bad name");
        launcher.focus = Field::Count;
        assert!(press(&mut launcher, KeyCode::Enter).is_none());
        assert_eq!(launcher.focus, Field::Name);
        launcher.name = "ok".into();
        launcher.focus = Field::Folder;
        launcher.insert("/no/such/folder");
        assert!(press(&mut launcher, KeyCode::Enter).is_none());
        assert_eq!(launcher.focus, Field::Folder);
        assert!(launcher.error.contains("No such directory"));
        assert!(matches!(
            press(&mut launcher, KeyCode::Esc),
            Some(Outcome::Cancel)
        ));
    }

    #[test]
    fn worker_count_stays_within_the_card_limit() {
        let mut launcher = launcher();
        launcher.focus = Field::Count;
        for _ in 0..20 {
            press(&mut launcher, KeyCode::Char('+'));
        }
        assert_eq!(launcher.workers.len(), MAX_WORKERS);
        for _ in 0..20 {
            press(&mut launcher, KeyCode::Char('-'));
        }
        assert!(launcher.workers.is_empty());
        launcher.name = "solo".into();
        assert!(launch(&mut launcher).worker_agents.is_empty());
    }

    #[test]
    fn folder_tab_completes_a_unique_directory() {
        let root = env::temp_dir().join(format!("launcher-complete-{}", std::process::id()));
        fs::create_dir_all(root.join("alpha-one")).unwrap();
        fs::create_dir_all(root.join("beta")).unwrap();
        let base = root.to_str().unwrap();
        assert_eq!(
            complete(&format!("{base}/al")).unwrap(),
            format!("{base}/alpha-one/")
        );
        assert!(complete(&format!("{base}/zz")).is_none());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn card_renders_choices_preview_and_resize_message() {
        let mut launcher = launcher();
        launcher.insert("auth");
        for (width, height, expected) in [
            (100, 30, "new team · dry run"),
            (100, 30, "◉ claude"),
            (100, 30, "herdr-shepherd auth --cwd"),
            (100, 30, "none"),
            (40, 10, "Resize the terminal"),
        ] {
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            terminal.draw(|frame| launcher.draw(frame)).unwrap();
            let text: String = terminal
                .backend()
                .buffer()
                .content
                .iter()
                .map(|cell| cell.symbol())
                .collect();
            assert!(text.contains(expected), "{expected}");
        }
    }
}
