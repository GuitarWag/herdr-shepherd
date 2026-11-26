use crate::{Result, package_root, process, string, template};
use std::{
    env,
    path::PathBuf,
    process::Command,
    thread,
    time::{Duration, Instant},
};

const HELP: &str = "usage: herdr-shepherd [<task>] [options]
       herdr-shepherd
       herdr-shepherd --interactive [--dry-run]
       herdr-shepherd --install [check]
       hs --template NAME [--branch-name BRANCH] [--destination PATH]
  -h, --help                     Show this help.
  --interactive                  Open the launcher. Piped input uses text prompts.
  --add-worker                   Use the current herdr pane as a worker.
  --director                     Start a director that manages bosses, with no workers.
  --under NAME                   Report this boss to the director NAME-director.
  --team                         Show the team across tabs.
  --json                         Use JSON with --team.
  --send boss|wN|NAME            Store a message and point the agent at it.
  --message TEXT                 The message for --send. Default: standard input.
  --read [ID]                    Print a message, or list unread messages for this pane.
  --boss-agent AGENT[:MODEL]     claude, codex, pi or opencode. Default: claude.
  --worker-agent AGENT[:MODEL]   Repeat for an ordered list of workers.
  --workers N                    0 through 9999. Default: 3, or the worker list length.
  --cwd PATH                     Default: the current directory.
  --template NAME                Start the team from a template in
                                 ~/.config/herdr-shepherd/templates.yaml.
  --branch-name BRANCH           Start the team in a new git worktree on BRANCH.
  --destination PATH             The worktree folder. Default: <repo>-<branch>.
  --boss-model M                 Override the boss model.
  --worker-model M               Override every worker model.
  --model M                      Override all models.
  --dry-run                      Print commands without creating panes.
  --version                      Print the package version.
  --install [check]              Install or check the command and skill links.

One --worker-agent applies to all workers. Multiple entries define each worker.
A model in an agent entry overrides --boss-model, --worker-model, and --model.
No arguments opens the board when input and output use a terminal.
An explicit worker count must match a list with multiple entries.
--team accepts only --json. --add-worker rejects boss options and --workers.
--team, --add-worker, --send and --read infer the task from the boss in the current folder.
Messages live in ~/.local/state/herdr-shepherd/mail/<task>/ for seven days.
A new team with the same task name starts with an empty mailbox.
--director uses --boss-agent and --boss-model and rejects worker options.
Running agents cannot change their model, folder, or agent type when they join.
Claude defaults: fable for the boss, opus for workers.
Codex defaults: gpt-6-astra for the boss, gpt-6.1-sol for workers.
pi uses its configured default model. opencode uses the model key in its own config.
A template has repo, and optional base, boss and workers. Options override it.
--branch-name reuses a branch from local or origin, or makes it from the base.
The task name defaults to the branch name, and then to the template name.";

#[derive(Clone, Copy, PartialEq)]
pub enum Operation {
    Create,
    Join,
    Team,
    Send,
    Read,
}

pub struct Options {
    pub task: String,
    pub cwd: PathBuf,
    pub operation: Operation,
    pub dry: bool,
    pub json: bool,
    pub boss_agent: String,
    pub worker_agents: Vec<String>,
    /// The model from each --worker-agent entry. An empty string means no entry model.
    pub worker_models: Vec<String>,
    pub boss_model: String,
    pub worker_model: String,
    pub change_directory: bool,
    /// The --send target and the --read message ID.
    pub to: String,
    pub message: Option<String>,
    pub message_id: Option<String>,
    pub director: bool,
    /// The task of the director that this boss reports to.
    pub under: String,
    pub worktree: Option<template::Worktree>,
}

/// The agent commands that shepherd starts. herdr reports each agent under the same name.
pub const AGENTS: [&str; 4] = ["claude", "codex", "pi", "opencode"];

pub fn agent(value: &str) -> Result<()> {
    if AGENTS.contains(&value) {
        Ok(())
    } else {
        Err(format!("unsupported agent: {value}").into())
    }
}

/// Split `AGENT[:MODEL]`. The agent rejects an unknown model itself.
pub fn agent_entry(value: &str) -> Result<(String, String)> {
    let (name, model) = value.split_once(':').unwrap_or((value, ""));
    agent(name)?;
    if value.contains(':') && model.is_empty() {
        return Err(format!("The agent entry {value} has no model after the colon.").into());
    }
    Ok((name.to_owned(), model.to_owned()))
}

pub fn task_name(value: &str) -> Result<()> {
    // A leading dot allows `..`, which leaves the mail folder.
    if !value.starts_with(|c: char| c.is_ascii_alphanumeric())
        || !value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
    {
        return Err(
            "The task name starts with a letter or digit and accepts dot, dash and underscore."
                .into(),
        );
    }
    Ok(())
}

pub fn worker_count(value: &str) -> Result<usize> {
    if value.is_empty() || value.len() > 4 || !value.bytes().all(|b| b.is_ascii_digit()) {
        return Err("--workers accepts a whole number from 0 through 9999".into());
    }
    Ok(value.parse()?)
}

pub fn parse(args: &[String]) -> Result<Option<Options>> {
    let mut options = Options {
        task: String::new(),
        cwd: env::current_dir()?,
        operation: Operation::Create,
        dry: false,
        json: false,
        boss_agent: "claude".into(),
        worker_agents: Vec::new(),
        worker_models: Vec::new(),
        boss_model: String::new(),
        worker_model: String::new(),
        change_directory: false,
        to: String::new(),
        message: None,
        message_id: None,
        director: false,
        under: String::new(),
        worktree: None,
    };
    let mut template_name = String::new();
    let mut branch = String::new();
    let mut destination = String::new();
    let mut workers: Option<String> = None;
    let mut boss_entry = String::new();
    let mut worker_entries = Vec::new();
    let mut create_options = false;
    let mut worker_options = false;
    let mut index = 0;
    while index < args.len() {
        let option = args[index].as_str();
        match option {
            "--workers" | "--cwd" | "--boss-agent" | "--worker-agent" | "--boss-model"
            | "--worker-model" | "--model" | "--under" | "--template" | "--branch-name"
            | "--destination" => {
                index += 1;
                let value = args
                    .get(index)
                    .filter(|value| !value.is_empty() && !value.starts_with("--"))
                    .ok_or_else(|| format!("{option} requires a value"))?;
                match option {
                    "--workers" | "--boss-agent" | "--boss-model" | "--under" | "--template"
                    | "--branch-name" | "--destination" => create_options = true,
                    "--cwd" => options.change_directory = true,
                    _ => worker_options = true,
                }
                match option {
                    "--workers" => workers = Some(value.clone()),
                    "--cwd" => options.cwd = value.into(),
                    "--boss-agent" => boss_entry = value.clone(),
                    "--worker-agent" => worker_entries.push(value.clone()),
                    "--boss-model" => options.boss_model = value.clone(),
                    "--worker-model" => options.worker_model = value.clone(),
                    "--under" => options.under = value.clone(),
                    "--template" => template_name = value.clone(),
                    "--branch-name" => branch = value.clone(),
                    "--destination" => destination = value.clone(),
                    _ => {
                        options.boss_model = value.clone();
                        options.worker_model = value.clone();
                    }
                }
            }
            "--team" | "--add-worker" | "--send" | "--read" => {
                if options.operation != Operation::Create {
                    return Err("select one of --add-worker, --team, --send and --read".into());
                }
                options.operation = match option {
                    "--team" => Operation::Team,
                    "--add-worker" => Operation::Join,
                    "--send" => {
                        index += 1;
                        options.to = args
                            .get(index)
                            .filter(|value| !value.is_empty() && !value.starts_with('-'))
                            .ok_or("--send requires a target: boss, wN or an agent name")?
                            .clone();
                        Operation::Send
                    }
                    _ => {
                        if let Some(value) = args
                            .get(index + 1)
                            .filter(|value| !value.is_empty() && !value.starts_with('-'))
                        {
                            index += 1;
                            options.message_id = Some(value.clone());
                        }
                        Operation::Read
                    }
                };
            }
            "--message" => {
                index += 1;
                let value = args.get(index).ok_or("--message requires a value")?;
                options.message = Some(value.clone());
            }
            "--director" => options.director = true,
            "--json" => options.json = true,
            "--dry-run" => options.dry = true,
            "--version" => {
                println!("herdr-shepherd {}", env!("CARGO_PKG_VERSION"));
                return Ok(None);
            }
            "--help" | "-h" => {
                println!("{HELP}");
                return Ok(None);
            }
            value if value.starts_with('-') => {
                return Err(format!("unknown option: {value}").into());
            }
            value if options.task.is_empty() => options.task = value.into(),
            _ => return Err("supply one task name".into()),
        }
        index += 1;
    }
    let mut base = None;
    if !template_name.is_empty() {
        if options.change_directory {
            return Err("--template sets the folder. Do not pass --cwd.".into());
        }
        let template = template::load(&template_name)?;
        options.cwd = template::expand_home(&template.repo)?;
        base = template.base;
        if boss_entry.is_empty() {
            boss_entry = template.boss.unwrap_or_default();
        }
        if let Some(entries) = template.workers.filter(|_| worker_entries.is_empty()) {
            workers.get_or_insert(entries.len().to_string());
            worker_entries = entries;
        }
    }
    if !destination.is_empty() && branch.is_empty() {
        return Err("--destination requires --branch-name".into());
    }
    if options.task.is_empty() && options.operation == Operation::Create {
        options.task = if !branch.is_empty() {
            template::slug(&branch)
        } else {
            template_name.clone()
        };
        if options.task.is_empty() {
            return Err(HELP.into());
        }
    }
    if !options.task.is_empty() {
        task_name(&options.task)?;
    }
    if !boss_entry.is_empty() {
        let (name, model) = agent_entry(&boss_entry)?;
        options.boss_agent = name;
        if !model.is_empty() {
            options.boss_model = model;
        }
    }
    for entry in &worker_entries {
        let (name, model) = agent_entry(entry)?;
        options.worker_agents.push(name);
        options.worker_models.push(model);
    }
    if !options.cwd.is_dir() {
        return Err(format!("no such directory: {}", options.cwd.display()).into());
    }
    options.cwd = options.cwd.canonicalize()?;
    if !branch.is_empty() {
        let path = if destination.is_empty() {
            let name = options
                .cwd
                .file_name()
                .ok_or("The repo folder has no name.")?;
            options.cwd.with_file_name(format!(
                "{}-{}",
                name.to_string_lossy(),
                template::slug(&branch)
            ))
        } else {
            env::current_dir()?.join(template::expand_home(&destination)?)
        };
        options.worktree = Some(template::Worktree {
            repo: std::mem::replace(&mut options.cwd, path.clone()),
            base,
            branch,
            path,
        });
    }
    if options.json && options.operation != Operation::Team {
        return Err("--json requires --team".into());
    }
    if options.director && options.operation != Operation::Create {
        return Err("--director starts a new director".into());
    }
    if !options.under.is_empty() {
        task_name(&options.under)?;
        if options.under == options.task {
            return Err("--under names another task, not this team".into());
        }
        if options.director {
            return Err("--under requires a boss, not --director".into());
        }
    }
    if options.message.is_some() && options.operation != Operation::Send {
        return Err("--message requires --send".into());
    }
    match options.operation {
        Operation::Team
            if create_options || worker_options || options.change_directory || options.dry =>
        {
            return Err("--team accepts only --json".into());
        }
        Operation::Send | Operation::Read if create_options || worker_options => {
            return Err("--send and --read accept only --cwd, --message and --dry-run".into());
        }
        Operation::Join => {
            if create_options {
                return Err("--add-worker does not accept boss options or --workers".into());
            }
            if options.worker_agents.len() > 1 {
                return Err("--add-worker accepts one --worker-agent".into());
            }
        }
        Operation::Create if options.director => {
            if workers.is_some() || worker_options {
                return Err("--director accepts no workers or worker options".into());
            }
        }
        Operation::Create => {
            let count = if let Some(value) = workers {
                worker_count(&value)?
            } else if options.worker_agents.len() > 1 {
                options.worker_agents.len()
            } else {
                3
            };
            if count > 9999 {
                return Err("The worker count exceeds 9999.".into());
            }
            if options.worker_agents.len() > 1 && options.worker_agents.len() != count {
                return Err("--workers must match the number of --worker-agent entries".into());
            }
            if options.worker_agents.len() <= 1 {
                let selected = options
                    .worker_agents
                    .first()
                    .cloned()
                    .unwrap_or_else(|| "claude".into());
                let model = options.worker_models.first().cloned().unwrap_or_default();
                options.worker_agents = vec![selected; count];
                options.worker_models = vec![model; count];
            }
        }
        _ => {}
    }
    if matches!(options.operation, Operation::Create | Operation::Join) {
        check_model(&options.boss_agent, &options.boss_model)?;
        for (index, agent) in options.worker_agents.iter().enumerate() {
            check_model(agent, options.worker_model(index))?;
        }
    }
    Ok(Some(options))
}

/// The opencode 2 terminal interface sends its first prompt through a shared
/// background service, which uses the `model` key from the opencode config.
/// Neither a command option nor OPENCODE_CONFIG_CONTENT changes that model.
fn check_model(agent: &str, model: &str) -> Result<()> {
    if agent == "opencode" && !model.is_empty() {
        return Err(format!(
            "opencode uses the model key in ~/.config/opencode/opencode.json. Remove the model {model} from the opencode agent, and do not use --model or --worker-model with an opencode worker."
        )
        .into());
    }
    Ok(())
}

impl Options {
    /// The entry model wins over --worker-model and --model.
    pub fn worker_model(&self, index: usize) -> &str {
        match self.worker_models.get(index) {
            Some(model) if !model.is_empty() => model,
            _ => &self.worker_model,
        }
    }
}

/// The model for an agent role when no option selects one.
/// An empty string lets the agent use its own configured default.
pub fn default_model(agent: &str, lead: bool) -> &'static str {
    match (agent, lead) {
        ("claude", true) => "fable",
        ("claude", false) => "opus",
        ("codex", true) => "gpt-6-astra",
        ("codex", false) => "gpt-6.1-sol",
        _ => "",
    }
}

pub fn launch_args(options: &Options, agent: &str, name: &str, model: &str) -> Result<Vec<String>> {
    let director = options.director;
    let boss = director || name == format!("{}-boss", options.task);
    let skill = package_root()?.join("skills/herdr-shepherd/SKILL.md");
    let mut prompt = format!(
        "Read the herdr-shepherd skill at {}. You are {name}.",
        skill.display()
    );
    if director {
        prompt += &format!(
            " You are the director for {task}. You manage bosses, not workers. \
Start a team with: herdr-shepherd TASK --cwd PATH --under {task}. \
Send decisions with: herdr-shepherd {task} --send TASK-boss. \
Do not wait for a boss to finish. Do not send messages to workers. \
List your messages with: herdr-shepherd {task} --read. Wait for the user's task.",
            task = options.task
        );
    } else if boss {
        prompt += &format!(
            " You are the boss for {} with {} workers named {}-wN. Wait for the user's task.",
            options.task,
            options.worker_agents.len(),
            options.task
        );
        if !options.under.is_empty() {
            prompt += &format!(
                " Your director is {under}-director. \
Send milestones, blockers and results with: herdr-shepherd {task} --send {under}-director. \
Run herdr-shepherd {task} --read between worker rounds.",
                under = options.under,
                task = options.task
            );
        }
    } else {
        prompt += &format!(
            " Your boss is {task}-boss. End your turn now. The boss sends your task as input in this pane. \
Do not poll for messages and do not run wait loops. \
Report each result with: herdr-shepherd {task} --send boss --message \"RESULT\".",
            task = options.task
        );
    }
    let model = if model.is_empty() {
        default_model(agent, boss)
    } else {
        model
    };
    let mut args = Vec::new();
    args.push(agent.to_owned());
    match agent {
        "claude" | "pi" => args.extend(["--name".into(), name.into()]),
        "codex" => args.push("--approve-for-me".into()),
        _ => args.push("--auto".into()),
    }
    if agent == "opencode" {
        args.push("--prompt".into());
    } else if !model.is_empty() {
        args.extend(["--model".into(), model.into()]);
    }
    args.push(prompt);
    Ok(args)
}

/// The most worker rows in one column. More workers add columns.
const COLUMN_ROWS: usize = 4;

/// The splits for `count` worker panes, in creation order, as
/// (worker index, parent worker or None for the boss, direction, ratio).
/// herdr gives the parent pane `ratio` of the space. Workers fill the columns
/// in order, and each column and each row gets an equal share.
fn layout(count: usize) -> Vec<(usize, Option<usize>, &'static str, f64)> {
    let columns = count.div_ceil(COLUMN_ROWS);
    // The first `count % columns` columns hold one more worker.
    let rows = |column: usize| count / columns + usize::from(column < count % columns);
    let first = |column: usize| (0..column).map(rows).sum::<usize>();
    let mut splits = Vec::new();
    for column in 0..columns {
        let (parent, ratio) = match column {
            0 => (None, 0.35),
            _ => (Some(first(column - 1)), 1.0 / (columns - column + 1) as f64),
        };
        splits.push((first(column), parent, "right", ratio));
    }
    for column in 0..columns {
        let top = first(column);
        for row in 1..rows(column) {
            let ratio = 1.0 / (rows(column) - row + 1) as f64;
            splits.push((top + row, Some(top + row - 1), "down", ratio));
        }
    }
    splits
}

/// The shell discards input that arrives before its line editor starts.
/// A rendered prompt shows that the line editor is ready.
pub fn wait_for_prompt(pane: &str) -> Result<()> {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        let screen = process::output(
            Command::new("herdr").args(["pane", "read", pane, "--source", "visible"]),
            Duration::from_secs(10),
        )?;
        if screen.status.success() && !screen.stdout.iter().all(u8::is_ascii_whitespace) {
            return Ok(());
        }
        if Instant::now() >= deadline {
            eprintln!("warning: the pane {pane} shows no shell prompt after 15 seconds.");
            return Ok(());
        }
        thread::sleep(Duration::from_millis(100));
    }
}

fn launch(options: &Options, pane: &str, agent: &str, name: &str, model: &str) -> Result<()> {
    let command = process::shell_command(&launch_args(options, agent, name, model)?);
    if !options.dry {
        wait_for_prompt(pane)?;
    }
    process::run(options.dry, &["pane", "send-text", pane, &command])?;
    process::run(options.dry, &["pane", "send-keys", pane, "Enter"])
}

/// Name the agent in a pane after it starts.
/// herdr knows an agent only once its process runs, so the first tries can fail with "not found".
fn rename_agent(options: &Options, pane: &str, name: &str) -> Result<()> {
    let args = ["agent", "rename", pane, name];
    if options.dry {
        return process::run(true, &args);
    }
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        match process::run(false, &args) {
            Err(error) if error.to_string().contains("not found") && Instant::now() < deadline => {
                thread::sleep(Duration::from_secs(1));
            }
            result => return result,
        }
    }
}

pub fn create(options: &Options) -> Result<()> {
    package_root()?;
    if !options.dry {
        for program in std::iter::once("herdr")
            .chain(std::iter::once(options.boss_agent.as_str()))
            .chain(options.worker_agents.iter().map(String::as_str))
            .chain(options.worktree.as_ref().map(|_| "git"))
        {
            if !process::available(program) {
                return Err(format!("{program} is not on PATH").into());
            }
        }
    }
    if !options.dry {
        // Every team of one task shares a mailbox, and launch clears it.
        let agents = process::herdr(&["agent", "list"])?;
        for role in ["boss", "director"] {
            let other = format!("{}-{role}", options.task);
            if agents["agents"]
                .as_array()
                .is_some_and(|agents| agents.iter().any(|agent| agent["name"] == other))
            {
                return Err(format!(
                    "{other} already uses the task {}. Choose another task name.",
                    options.task
                )
                .into());
            }
        }
    }
    if let Some(worktree) = &options.worktree {
        worktree.create(options.dry)?;
    }
    let cwd = options
        .cwd
        .to_str()
        .ok_or("The folder path must use UTF-8.")?;
    let tab_args = [
        "tab",
        "create",
        "--label",
        &options.task,
        "--cwd",
        cwd,
        "--focus",
    ];
    let boss = if options.dry {
        println!("Dry run. No panes are created.");
        process::run(true, &tab_args)?;
        "<boss>".to_owned()
    } else {
        string(&process::herdr(&tab_args)?["root_pane"], "pane_id")?.to_owned()
    };
    if !options.dry {
        crate::mail::clear(&options.task)?;
        crate::view::set_director(&options.task, &options.under)?;
    }
    let role = if options.director { "director" } else { "boss" };
    let name = format!("{}-{role}", options.task);
    process::run(options.dry, &["pane", "rename", &boss, &name])?;
    launch(
        options,
        &boss,
        &options.boss_agent,
        &name,
        &options.boss_model,
    )?;
    rename_agent(options, &boss, &name)?;
    println!("{role}  {boss}  {}", options.boss_agent);
    // Create every pane first, then start the agents in worker order.
    let mut panes: Vec<String> = Vec::new();
    for (index, parent, direction, ratio) in layout(options.worker_agents.len()) {
        let source = parent.map_or(boss.as_str(), |parent| panes[parent].as_str());
        let ratio = format!("{ratio:.4}");
        let split = [
            "pane",
            "split",
            source,
            "--direction",
            direction,
            "--ratio",
            &ratio,
            "--cwd",
            cwd,
            "--no-focus",
        ];
        let pane = if options.dry {
            process::run(true, &split)?;
            format!("<w{}>", index + 1)
        } else {
            string(&process::herdr(&split)?["pane"], "pane_id")?.to_owned()
        };
        if panes.len() <= index {
            panes.resize(index + 1, String::new());
        }
        panes[index] = pane;
    }
    for (index, agent) in options.worker_agents.iter().enumerate() {
        let pane = &panes[index];
        let name = format!("{}-w{}", options.task, index + 1);
        process::run(options.dry, &["pane", "rename", pane, &name])?;
        launch(options, pane, agent, &name, options.worker_model(index))?;
        rename_agent(options, pane, &name)?;
        println!("w{}    {pane}  {agent}", index + 1);
    }
    // w1 is in the first column, so its left neighbor is the boss.
    let previous = panes.first().cloned().unwrap_or(boss);
    if !options.dry {
        let _ = process::herdr(&["pane", "focus", "--direction", "left", "--pane", &previous]);
        if options.director {
            println!("The director runs {}.", options.boss_agent);
        } else if options.worker_agents.is_empty() {
            println!("No workers. The boss runs {}.", options.boss_agent);
        } else {
            println!(
                "Workers: {}-w1 through {}-w{}. Use the herdr-shepherd skill to send work.",
                options.task,
                options.task,
                options.worker_agents.len()
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::layout;

    #[test]
    fn layout_gives_columns_of_equal_rows() {
        assert!(layout(0).is_empty());
        let three = layout(3);
        assert_eq!(three[0], (0, None, "right", 0.35));
        assert_eq!(three[1], (1, Some(0), "down", 1.0 / 3.0));
        assert_eq!(three[2], (2, Some(1), "down", 0.5));
        let eight = layout(8);
        assert_eq!(
            eight[..2],
            [(0, None, "right", 0.35), (4, Some(0), "right", 0.5)]
        );
        assert_eq!(eight[2], (1, Some(0), "down", 0.25));
        assert_eq!(eight[5], (5, Some(4), "down", 0.25));
        assert_eq!(eight.len(), 8);
        // Nine workers use three columns of three rows.
        let tops: Vec<_> = layout(9)
            .iter()
            .filter(|s| s.2 == "right")
            .map(|s| s.0)
            .collect();
        assert_eq!(tops, [0, 3, 6]);
    }
}
