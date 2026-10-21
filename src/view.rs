use crate::{Result, optional, process, string};
use serde_json::{Value, json};
use std::{collections::BTreeMap, env, path::Path, process::Command, thread, time::Duration};

pub fn team_name(name: &str) -> Option<(&str, &str)> {
    let (subject, role) = name.rsplit_once('-')?;
    if subject.is_empty() {
        return None;
    }
    if role == "boss"
        || role.strip_prefix('w').is_some_and(|number| {
            !number.is_empty()
                && !number.starts_with('0')
                && number.bytes().all(|b| b.is_ascii_digit())
        })
    {
        Some((subject, role))
    } else {
        None
    }
}

fn role(agent: &Value) -> &str {
    team_name(optional(agent, "name"))
        .map(|(_, role)| role)
        .unwrap_or_else(|| default(optional(agent, "agent"), "?"))
}

fn default<'a>(value: &'a str, fallback: &'a str) -> &'a str {
    if value.is_empty() { fallback } else { value }
}

fn repo(cwd: &str) -> Option<String> {
    if cwd.is_empty() {
        return None;
    }
    let output = process::output(
        Command::new("git").args(["-C", cwd, "remote", "get-url", "origin"]),
        Duration::from_secs(5),
    )
    .ok()?;
    if !output.status.success() {
        return None;
    }
    let url = String::from_utf8(output.stdout).ok()?;
    let url = url
        .trim()
        .trim_end_matches('/')
        .trim_end_matches(".git")
        .replace(':', "/");
    if url.is_empty() {
        return None;
    }
    let parts: Vec<_> = url.split('/').collect();
    Some(parts[parts.len().saturating_sub(2)..].join("/"))
}

fn groups(subject: Option<&str>) -> Result<Vec<Value>> {
    let agents = process::herdr(&["agent", "list"])?;
    let agents = agents["agents"]
        .as_array()
        .ok_or("The herdr agent list is missing.")?;
    let tabs = process::herdr(&["tab", "list"])?;
    let tabs = tabs["tabs"]
        .as_array()
        .ok_or("The herdr tab list is missing.")?;
    let mut groups: BTreeMap<(String, &str), Vec<&Value>> = BTreeMap::new();
    for agent in agents {
        let (key, kind) = if let Some((name, _)) = team_name(optional(agent, "name")) {
            (name.to_owned(), "herd")
        } else {
            let tab = string(agent, "tab_id")?;
            let label = tabs
                .iter()
                .find(|entry| optional(entry, "tab_id") == tab)
                .map(|entry| optional(entry, "label"))
                .unwrap_or("");
            let key = if !label.is_empty() && !label.chars().all(char::is_numeric) {
                label
            } else {
                Path::new(optional(agent, "cwd"))
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or(tab)
            };
            (key.to_owned(), "session")
        };
        groups.entry((key, kind)).or_default().push(agent);
    }
    let mut output = Vec::new();
    for ((name, kind), mut agents) in groups {
        if subject.is_some_and(|subject| subject != name) {
            continue;
        }
        agents.sort_by_key(|agent| {
            (
                role(agent) != "boss",
                default(optional(agent, "name"), optional(agent, "pane_id")),
            )
        });
        let cwd = optional(agents[0], "cwd");
        let mut members = Vec::new();
        for agent in &agents {
            let pane = string(agent, "pane_id")?;
            members.push(json!({ "name": default(optional(agent, "name"), pane), "role": role(agent),
                "agent": default(optional(agent, "agent"), "?"), "status": default(optional(agent, "agent_status"), "unknown"),
                "pane_id": pane, "cwd": optional(agent, "cwd") }));
        }
        let display = repo(cwd).unwrap_or_else(|| match env::var("HOME") {
            Ok(home) if !home.is_empty() => cwd.replace(&home, "~"),
            _ => cwd.to_owned(),
        });
        output.push(json!({ "subject": name, "kind": kind, "cwd": cwd, "repo": display,
            "blocked": agents.iter().filter(|agent| optional(agent, "agent_status") == "blocked").count(),
            "working": agents.iter().filter(|agent| optional(agent, "agent_status") == "working").count(),
            "members": members }));
    }
    output.sort_by(|a, b| {
        (optional(a, "kind") != "herd")
            .cmp(&(optional(b, "kind") != "herd"))
            .then_with(|| b["blocked"].as_u64().cmp(&a["blocked"].as_u64()))
            .then_with(|| b["working"].as_u64().cmp(&a["working"].as_u64()))
            .then_with(|| optional(a, "subject").cmp(optional(b, "subject")))
    });
    Ok(output)
}

pub fn show(subject: Option<&str>, json: bool) -> Result<()> {
    let groups = groups(subject)?;
    if json {
        println!("{}", serde_json::to_string(&groups)?);
        return Ok(());
    }
    if groups.is_empty() {
        println!("\x1b[2mnothing running.\x1b[0m");
    }
    for group in groups {
        let mark = if optional(&group, "kind") == "herd" {
            ""
        } else {
            " \x1b[2msession\x1b[0m"
        };
        let blocked = group["blocked"].as_u64().unwrap_or(0);
        let flag = if blocked > 0 {
            format!("  \x1b[31m{blocked} blocked\x1b[0m")
        } else {
            String::new()
        };
        println!(
            "\n\x1b[1m{}\x1b[0m{mark}  \x1b[2m{}\x1b[0m{flag}",
            optional(&group, "subject"),
            optional(&group, "repo")
        );
        if let Some(members) = group["members"].as_array() {
            for member in members {
                let status = optional(member, "status");
                let color = match status {
                    "idle" => "\x1b[32m",
                    "working" => "\x1b[33m",
                    "blocked" => "\x1b[31m",
                    "done" => "\x1b[36m",
                    "unknown" => "\x1b[2m",
                    _ => "",
                };
                println!(
                    "  {:<8} {:<8} {color}{status:<8}\x1b[0m \x1b[2m{}\x1b[0m",
                    optional(member, "role"),
                    optional(member, "agent"),
                    optional(member, "pane_id")
                );
            }
        }
    }
    Ok(())
}

pub fn run(args: &[String]) -> Result<()> {
    let mut subject = None;
    let mut json = false;
    let mut watch = false;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--subject" => {
                index += 1;
                subject = Some(
                    args.get(index)
                        .filter(|value| !value.starts_with("--"))
                        .ok_or("--subject requires a value")?
                        .as_str(),
                );
            }
            "--json" => json = true,
            "--watch" => watch = true,
            "--version" => {
                println!("herd-view {}", env!("CARGO_PKG_VERSION"));
                return Ok(());
            }
            "--help" | "-h" => {
                println!("usage: herd-view [--subject TASK] [--json] [--watch] [--version]");
                return Ok(());
            }
            value => return Err(format!("unknown option: {value}").into()),
        }
        index += 1;
    }
    loop {
        if watch && !json {
            print!("\x1b[2J\x1b[H");
        }
        show(subject, json)?;
        if !watch || json {
            return Ok(());
        }
        thread::sleep(Duration::from_secs(3));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_require_a_complete_team_role() {
        assert_eq!(team_name("my-task-w12"), Some(("my-task", "w12")));
        assert_eq!(team_name("my-task-boss"), Some(("my-task", "boss")));
        for name in ["api-review", "task-w0", "task-w01", "task-w1x", "-boss"] {
            assert_eq!(team_name(name), None);
        }
    }
}
