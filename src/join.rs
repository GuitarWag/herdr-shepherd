use crate::{
    Result,
    cli::{self, Options},
    optional, package_root, process, string, view,
};
use sha2::{Digest, Sha256};
use std::{
    env,
    fs::{File, OpenOptions},
    os::unix::{
        fs::{MetadataExt, OpenOptionsExt},
        process::CommandExt,
    },
    path::Path,
    process::Command,
};

fn registration_lock(task: &str) -> Result<File> {
    let socket = env::var("HERDR_SOCKET_PATH").unwrap_or_default();
    let digest = Sha256::digest(format!("{socket}\0{task}").as_bytes());
    // Match the previous launcher's lock name so both versions serialize joins.
    let uid = unsafe { libc::getuid() };
    let path = env::temp_dir().join(format!("herd-join-{uid}-{digest:x}.lock"));
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)?;
    let metadata = lock.metadata()?;
    if metadata.uid() != uid || !metadata.is_file() {
        return Err(
            "The worker registration lock has a different owner or is not a regular file.".into(),
        );
    }
    lock.lock()?;
    Ok(lock)
}

fn worker_number(name: &str, task: &str) -> Result<Option<u64>> {
    if let Some((subject, role)) = view::team_name(name)
        && subject == task
        && role.starts_with('w')
    {
        return Ok(Some(
            role[1..]
                .parse()
                .map_err(|_| "The worker number is too large.")?,
        ));
    }
    Ok(None)
}

pub fn run(options: &Options) -> Result<()> {
    package_root()?;
    let pane = env::var("HERDR_PANE_ID")
        .ok()
        .filter(|value| !value.is_empty())
        .ok_or("Run --add-worker from the target herdr pane.")?;
    let lock = registration_lock(&options.task)?;
    let response = process::herdr(&["agent", "list"])?;
    let agents = response["agents"]
        .as_array()
        .ok_or("The herdr agent list is missing.")?;
    let boss_name = format!("{}-boss", options.task);
    let bosses: Vec<_> = agents
        .iter()
        .filter(|agent| optional(agent, "name") == boss_name)
        .collect();
    if bosses.len() != 1 || !matches!(optional(bosses[0], "agent"), "claude" | "codex") {
        return Err(format!("Expected one running boss named {boss_name}.").into());
    }
    if string(bosses[0], "pane_id")? == pane {
        return Err("The boss pane cannot join as its own worker.".into());
    }
    let boss_cwd = optional(bosses[0], "cwd");
    if boss_cwd.is_empty() || Path::new(boss_cwd).canonicalize()? != options.cwd {
        return Err(format!("The worker folder must match the boss folder: {boss_cwd}").into());
    }
    let current = process::herdr(&["pane", "get", &pane])?;
    let current = &current["pane"];
    if string(current, "pane_id")? != pane {
        return Err("The current pane ID differs from HERDR_PANE_ID.".into());
    }
    let current_agent = optional(current, "agent");
    let requested = options
        .worker_agents
        .first()
        .map(String::as_str)
        .unwrap_or("");
    if !current_agent.is_empty() {
        let cwd = optional(current, "cwd");
        if cwd.is_empty() || Path::new(cwd).canonicalize()? != options.cwd {
            return Err(format!("The running worker must use the boss folder: {boss_cwd}").into());
        }
        cli::agent(current_agent)?;
        if !requested.is_empty() && requested != current_agent {
            return Err(format!("The current pane already runs {current_agent}.").into());
        }
        if !options.worker_model(0).is_empty() || options.change_directory {
            return Err("Model and directory options require a shell pane.".into());
        }
    }
    let selected = if !current_agent.is_empty() {
        current_agent
    } else if !requested.is_empty() {
        requested
    } else {
        "claude"
    };
    if current_agent.is_empty() && !options.dry && !process::available(selected) {
        return Err(format!("{selected} is not on PATH").into());
    }
    let registered = agents
        .iter()
        .find(|agent| optional(agent, "pane_id") == pane)
        .map(|agent| optional(agent, "name"))
        .unwrap_or("");
    let current_name = if registered.is_empty() {
        optional(current, "name")
    } else {
        registered
    };
    let name = if worker_number(current_name, &options.task)?.is_some() {
        current_name.to_owned()
    } else {
        if view::team_name(current_name).is_some() {
            return Err(format!(
                "The current pane already belongs to another team: {current_name}"
            )
            .into());
        }
        let mut maximum = 0;
        for agent in agents {
            maximum =
                maximum.max(worker_number(optional(agent, "name"), &options.task)?.unwrap_or(0));
        }
        format!(
            "{}-w{}",
            options.task,
            maximum
                .checked_add(1)
                .ok_or("The worker number is too large.")?
        )
    };
    if agents
        .iter()
        .any(|agent| optional(agent, "name") == name && optional(agent, "pane_id") != pane)
    {
        return Err(format!("The worker name is not unique: {name}").into());
    }
    let argv = cli::launch_args(options, selected, &name, options.worker_model(0))?;
    if !options.dry {
        process::herdr(&["agent", "rename", &pane, &name])?;
        process::herdr(&["pane", "rename", &pane, &name])?;
    }
    drop(lock);
    if options.dry {
        println!("Dry run. The current pane becomes {name}.");
    } else {
        println!("Worker {name} uses the current pane {pane}. Boss: {boss_name}.");
    }
    if !current_agent.is_empty() {
        println!("{}", argv.last().ok_or("The startup prompt is missing.")?);
    } else if options.dry {
        println!("  {}", process::shell_command(&argv));
    } else {
        return Err(Command::new(&argv[0])
            .args(&argv[1..])
            .current_dir(&options.cwd)
            .exec()
            .into());
    }
    Ok(())
}
