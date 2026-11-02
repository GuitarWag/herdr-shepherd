use crate::{Result, cli::Options, optional, process, string};
use serde_json::Value;
use std::{
    env, fs,
    path::PathBuf,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

const KEEP: Duration = Duration::from_secs(7 * 24 * 60 * 60);

fn folder(task: &str) -> Result<PathBuf> {
    crate::cli::task_name(task)?;
    Ok(process::home()?
        .join(".local/state/herdr-shepherd/mail")
        .join(task))
}

// ponytail: one file per message; add a sent list if bosses need delivery reports.
/// Open the mailbox and delete messages older than seven days.
fn mailbox(task: &str) -> Result<PathBuf> {
    let dir = folder(task)?;
    fs::create_dir_all(&dir)?;
    let now = SystemTime::now();
    for entry in fs::read_dir(&dir)?.flatten() {
        let stale = entry
            .metadata()
            .and_then(|data| data.modified())
            .is_ok_and(|modified| now.duration_since(modified).unwrap_or_default() > KEEP);
        if stale {
            let _ = fs::remove_file(entry.path());
        }
    }
    Ok(dir)
}

/// Remove every message of a task. A new team starts with an empty mailbox.
pub fn clear(task: &str) -> Result<()> {
    let dir = folder(task)?;
    if dir.is_dir() {
        fs::remove_dir_all(dir)?;
    }
    Ok(())
}

fn agents() -> Result<Vec<Value>> {
    let response = process::herdr(&["agent", "list"])?;
    response["agents"]
        .as_array()
        .cloned()
        .ok_or_else(|| "The herdr agent list is missing.".into())
}

/// The registered name of the current pane, or `user` from a pane without a name.
fn sender(agents: &[Value]) -> String {
    let pane = env::var("HERDR_PANE_ID").unwrap_or_default();
    agents
        .iter()
        .find(|agent| !pane.is_empty() && optional(agent, "pane_id") == pane)
        .map(|agent| optional(agent, "name"))
        .filter(|name| !name.is_empty())
        .unwrap_or("user")
        .to_owned()
}

/// Accept `boss`, `w1`, or a complete agent name.
fn member(task: &str, to: &str) -> String {
    let worker = to.len() > 1 && to.starts_with('w') && to[1..].bytes().all(|b| b.is_ascii_digit());
    if to == "boss" || worker {
        format!("{task}-{to}")
    } else {
        to.to_owned()
    }
}

fn message_id(value: &str) -> Result<()> {
    if value.is_empty()
        || !value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-')
    {
        return Err("The message ID accepts letters, digits and dash.".into());
    }
    Ok(())
}

pub fn send(options: &Options, to: &str, text: &str) -> Result<()> {
    let (id, name, path) = deliver(&options.task, to, text, options.dry)?;
    if options.dry {
        println!("Dry run. No message is stored.");
    } else {
        println!("Sent {id} to {name}: {}", path.display());
    }
    Ok(())
}

/// Store a message and point the agent at it. Returns the ID, target name and file.
/// `task` resolves `boss` and `wN`, and owns messages to agents outside a team.
pub fn deliver(task: &str, to: &str, text: &str, dry: bool) -> Result<(String, String, PathBuf)> {
    let text = text.trim();
    if text.is_empty() {
        return Err("The message is empty.".into());
    }
    let name = member(task, to);
    let agents = if dry { Vec::new() } else { agents()? };
    let pane = if dry {
        "<pane>".to_owned()
    } else {
        let target = agents
            .iter()
            .find(|agent| optional(agent, "name") == name)
            .ok_or_else(|| format!("No agent is named {name}."))?;
        string(target, "pane_id")?.to_owned()
    };
    let from = sender(&agents);
    let role = |name: &str| crate::view::team_name(name).map(|(_, role)| role.to_owned());
    if role(&from).as_deref() == Some("director")
        && !matches!(role(&name).as_deref(), Some("boss" | "director"))
    {
        return Err(format!("A director sends messages only to bosses, not to {name}.").into());
    }
    // The recipient team owns the message, so its --read list shows it.
    let task = crate::view::team_name(&name)
        .map(|(task, _)| task)
        .unwrap_or(task);
    let id = format!(
        "m{}-{}",
        SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis(),
        std::process::id()
    );
    let path = mailbox(task)?.join(format!("{id}.md"));
    if !dry {
        fs::write(&path, format!("From: {from}\nTo: {name}\n\n{text}\n"))?;
    }
    // The pointer stays short and plain, so the agent input receives it complete.
    let pointer = format!("Message {id} from {from}. Run: herdr-shepherd {task} --read {id}");
    process::run(dry, &["agent", "send", &name, &pointer])?;
    process::run(dry, &["pane", "send-keys", &pane, "Enter"])?;
    Ok((id, name, path))
}

/// The unread messages of a task for `me` as (ID, sender, first line).
/// The name `user` receives every message.
pub fn unread(task: &str, me: &str) -> Result<Vec<(String, String, String)>> {
    let dir = mailbox(task)?;
    let mut paths: Vec<_> = fs::read_dir(&dir)?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| {
            let name = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("");
            name.ends_with(".md") && !name.ends_with(".read.md")
        })
        .collect();
    paths.sort();
    let mut messages = Vec::new();
    for path in paths {
        let content = fs::read_to_string(&path)?;
        let mut lines = content.lines();
        let from = lines.next().unwrap_or("").trim_start_matches("From: ");
        let to = lines.next().unwrap_or("").trim_start_matches("To: ");
        if me != "user" && to != me {
            continue;
        }
        let first = lines.find(|line| !line.trim().is_empty()).unwrap_or("");
        let id = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or("");
        messages.push((id.to_owned(), from.to_owned(), first.to_owned()));
    }
    Ok(messages)
}

/// Return a message and mark it read.
pub fn open(task: &str, id: &str) -> Result<String> {
    let dir = mailbox(task)?;
    message_id(id)?;
    let unread = dir.join(format!("{id}.md"));
    let read = dir.join(format!("{id}.read.md"));
    if unread.is_file() {
        fs::rename(&unread, &read)?;
    } else if !read.is_file() {
        return Err(format!("No message {id} in {}.", dir.display()).into());
    }
    Ok(fs::read_to_string(read)?)
}

pub fn read(options: &Options, id: Option<&str>) -> Result<()> {
    if let Some(id) = id {
        print!("{}", open(&options.task, id)?);
        return Ok(());
    }
    let me = sender(&agents()?);
    let messages = unread(&options.task, &me)?;
    for (id, from, first) in &messages {
        println!("{id}  From: {from}  {first}");
    }
    if messages.is_empty() {
        println!("No unread messages for {me}.");
    }
    Ok(())
}
