use crate::{Result, package_root, process, string};
use std::{env, path::PathBuf, thread, time::Duration};

const HELP: &str = "usage: herdr-shepherd <task> [options]
  --add-worker                   Use the current herdr pane as a worker.
  --team                         Show the team across tabs.
  --json                         Use JSON with --team.
  --boss-agent claude|codex       Default: claude.
  --worker-agent claude|codex     Repeat for an ordered list of workers.
  --workers N                    0 through 9999. Default: 3, or the worker list length.
  --cwd PATH                     Default: the current directory.
  --boss-model M                 Override the boss model.
  --worker-model M               Override every worker model.
  --model M                      Override all models.
  --dry-run                      Print commands without creating panes.
  --version                      Print the package version.
  --install [check]              Install or check the command and skill links.

One --worker-agent applies to all workers. Multiple entries define each worker.
An explicit worker count must match a list with multiple entries.
Claude defaults: fable for the boss, opus for workers.
Codex defaults: gpt-6-astra for the boss, gpt-6.1-sol for workers.";

#[derive(Clone, Copy, PartialEq)]
pub enum Operation {
    Create,
    Join,
    Team,
}

pub struct Options {
    pub task: String,
    pub cwd: PathBuf,
    pub operation: Operation,
    pub dry: bool,
    pub json: bool,
    pub boss_agent: String,
    pub worker_agents: Vec<String>,
    pub boss_model: String,
    pub worker_model: String,
    pub change_directory: bool,
}

pub fn agent(value: &str) -> Result<()> {
    if matches!(value, "claude" | "codex") {
        Ok(())
    } else {
        Err(format!("unsupported agent: {value}").into())
    }
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
        boss_model: String::new(),
        worker_model: String::new(),
        change_directory: false,
    };
    let mut workers: Option<String> = None;
    let mut create_options = false;
    let mut worker_options = false;
    let mut index = 0;
    while index < args.len() {
        let option = args[index].as_str();
        match option {
            "--workers" | "--cwd" | "--boss-agent" | "--worker-agent" | "--boss-model"
            | "--worker-model" | "--model" => {
                index += 1;
                let value = args
                    .get(index)
                    .filter(|value| !value.is_empty() && !value.starts_with("--"))
                    .ok_or_else(|| format!("{option} requires a value"))?;
                match option {
                    "--workers" | "--boss-agent" | "--boss-model" => create_options = true,
                    "--cwd" => options.change_directory = true,
                    _ => worker_options = true,
                }
                match option {
                    "--workers" => workers = Some(value.clone()),
                    "--cwd" => options.cwd = value.into(),
                    "--boss-agent" => options.boss_agent = value.clone(),
                    "--worker-agent" => options.worker_agents.push(value.clone()),
                    "--boss-model" => options.boss_model = value.clone(),
                    "--worker-model" => options.worker_model = value.clone(),
                    _ => {
                        options.boss_model = value.clone();
                        options.worker_model = value.clone();
                    }
                }
            }
            "--team" | "--add-worker" => {
                if options.operation != Operation::Create {
                    return Err("select one of --add-worker and --team".into());
                }
                options.operation = if option == "--team" {
                    Operation::Team
                } else {
                    Operation::Join
                };
            }
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
    if options.task.is_empty() {
        return Err(HELP.into());
    }
    if !options
        .task
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
    {
        return Err("the task name accepts letters, digits, dot, dash and underscore".into());
    }
    if !options.cwd.is_dir() {
        return Err(format!("no such directory: {}", options.cwd.display()).into());
    }
    options.cwd = options.cwd.canonicalize()?;
    if options.json && options.operation != Operation::Team {
        return Err("--json requires --team".into());
    }
    match options.operation {
        Operation::Team
            if create_options || worker_options || options.change_directory || options.dry =>
        {
            return Err("--team accepts only --json".into());
        }
        Operation::Join => {
            if create_options {
                return Err("--add-worker does not accept boss options or --workers".into());
            }
            if options.worker_agents.len() > 1 {
                return Err("--add-worker accepts one --worker-agent".into());
            }
            for selected in &options.worker_agents {
                agent(selected)?;
            }
        }
        Operation::Create => {
            let count = if let Some(value) = workers {
                if value.is_empty() || value.len() > 4 || !value.bytes().all(|b| b.is_ascii_digit())
                {
                    return Err("--workers accepts a whole number from 0 through 9999".into());
                }
                value.parse::<usize>()?
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
            agent(&options.boss_agent)?;
            for selected in &options.worker_agents {
                agent(selected)?;
            }
            if options.worker_agents.len() <= 1 {
                let selected = options
                    .worker_agents
                    .first()
                    .cloned()
                    .unwrap_or_else(|| "claude".into());
                options.worker_agents = vec![selected; count];
            }
        }
        _ => {}
    }
    Ok(Some(options))
}

pub fn launch_args(options: &Options, agent: &str, name: &str, model: &str) -> Result<Vec<String>> {
    let boss = name == format!("{}-boss", options.task);
    let skill = package_root()?.join("skills/herdr-shepherd/SKILL.md");
    let mut prompt = format!(
        "Read the herdr-shepherd skill at {}. You are {name}.",
        skill.display()
    );
    if boss {
        prompt += &format!(
            " You are the boss for {} with {} workers named {}-wN. Wait for the user's task.",
            options.task,
            options.worker_agents.len(),
            options.task
        );
    } else {
        prompt += &format!(
            " Your boss is {}-boss. Wait for a task. Report results in this pane for the boss to read.",
            options.task
        );
    }
    let mut args = vec![agent.to_owned()];
    if agent == "claude" {
        args.extend(["--name".into(), name.into()]);
    } else {
        args.push("--approve-for-me".into());
    }
    let model = if !model.is_empty() {
        model
    } else {
        match (agent, boss) {
            ("claude", true) => "fable",
            ("claude", false) => "opus",
            (_, true) => "gpt-6-astra",
            _ => "gpt-6.1-sol",
        }
    };
    args.extend(["--model".into(), model.into(), prompt]);
    Ok(args)
}

fn launch(options: &Options, pane: &str, agent: &str, name: &str, model: &str) -> Result<()> {
    let command = process::shell_command(&launch_args(options, agent, name, model)?);
    // The pane shell needs time to start before it receives the command.
    if !options.dry {
        thread::sleep(Duration::from_secs(1));
    }
    process::run(options.dry, &["pane", "send-text", pane, &command])?;
    process::run(options.dry, &["pane", "send-keys", pane, "Enter"])
}

pub fn create(options: &Options) -> Result<()> {
    package_root()?;
    if !options.dry {
        for program in std::iter::once("herdr")
            .chain(std::iter::once(options.boss_agent.as_str()))
            .chain(options.worker_agents.iter().map(String::as_str))
        {
            if !process::available(program) {
                return Err(format!("{program} is not on PATH").into());
            }
        }
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
    let name = format!("{}-boss", options.task);
    process::run(options.dry, &["pane", "rename", &boss, &name])?;
    process::run(options.dry, &["agent", "rename", &boss, &name])?;
    launch(
        options,
        &boss,
        &options.boss_agent,
        &name,
        &options.boss_model,
    )?;
    println!("boss  {boss}  {}", options.boss_agent);
    let mut previous = boss;
    for (index, agent) in options.worker_agents.iter().enumerate() {
        let direction = if index == 0 { "right" } else { "down" };
        let ratio = if index == 0 { "0.35" } else { "0.5" };
        let split = [
            "pane",
            "split",
            &previous,
            "--direction",
            direction,
            "--ratio",
            ratio,
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
        let name = format!("{}-w{}", options.task, index + 1);
        process::run(options.dry, &["pane", "rename", &pane, &name])?;
        process::run(options.dry, &["agent", "rename", &pane, &name])?;
        launch(options, &pane, agent, &name, &options.worker_model)?;
        println!("w{}    {pane}  {agent}", index + 1);
        previous = pane;
    }
    if !options.dry {
        let _ = process::herdr(&["pane", "focus", "--direction", "left", "--pane", &previous]);
        if options.worker_agents.is_empty() {
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
