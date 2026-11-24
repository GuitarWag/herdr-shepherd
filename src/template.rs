use crate::{Result, process};
use serde::Deserialize;
use std::{collections::BTreeMap, fs, path::PathBuf, process::Command, time::Duration};

#[derive(Deserialize, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Template {
    pub repo: String,
    pub base: Option<String>,
    pub boss: Option<String>,
    pub workers: Option<Vec<String>>,
}

pub struct Worktree {
    pub repo: PathBuf,
    pub base: Option<String>,
    pub branch: String,
    pub path: PathBuf,
}

pub fn path() -> Result<PathBuf> {
    Ok(process::home()?.join(".config/herdr-shepherd/templates.yaml"))
}

fn parse(text: &str) -> Result<BTreeMap<String, Template>> {
    Ok(serde_norway::from_str(text)?)
}

pub fn load(name: &str) -> Result<Template> {
    let path = path()?;
    let text = fs::read_to_string(&path)
        .map_err(|error| format!("Cannot read {}: {error}", path.display()))?;
    let mut templates =
        parse(&text).map_err(|error| format!("Cannot parse {}: {error}", path.display()))?;
    templates.remove(name).ok_or_else(|| {
        let names: Vec<_> = templates.keys().map(String::as_str).collect();
        format!(
            "{} has no template {name}. Templates: {}",
            path.display(),
            names.join(", ")
        )
        .into()
    })
}

pub fn expand_home(value: &str) -> Result<PathBuf> {
    Ok(match value.strip_prefix("~/") {
        Some(rest) => process::home()?.join(rest),
        None if value == "~" => process::home()?,
        None => value.into(),
    })
}

/// A task name from a branch name: `feat/xyz` gives `feat-xyz`.
pub fn slug(branch: &str) -> String {
    branch
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || "._-".contains(c) {
                c
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_start_matches(['.', '-', '_'])
        .to_owned()
}

fn git(args: &[&str]) -> Result<std::process::Output> {
    process::output(Command::new("git").args(args), Duration::from_secs(300))
}

impl Worktree {
    /// Reuse a local branch that exists. Otherwise make it from the base.
    fn args(&self) -> Result<Vec<String>> {
        let repo = self.repo.to_str().ok_or("The repo path must use UTF-8.")?;
        let path = self
            .path
            .to_str()
            .ok_or("The worktree path must use UTF-8.")?;
        let exists = |reference: String| {
            process::available("git")
                && git(&["-C", repo, "rev-parse", "--verify", "--quiet", &reference])
                    .is_ok_and(|output| output.status.success())
        };
        let mut args: Vec<String> = ["-C", repo, "worktree", "add"].map(String::from).into();
        // git creates a local branch that tracks origin/<branch>.
        if exists(format!("refs/heads/{}", self.branch))
            || exists(format!("refs/remotes/origin/{}", self.branch))
        {
            args.extend(["--".into(), path.into(), self.branch.clone()]);
        } else {
            args.extend(["-b".into(), self.branch.clone(), "--".into(), path.into()]);
            args.extend(self.base.clone());
        }
        Ok(args)
    }

    pub fn create(&self, dry: bool) -> Result<()> {
        let args = self.args()?;
        if dry {
            println!("  git {}", process::shell_command(&args));
            return Ok(());
        }
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        let output = git(&args)?;
        if !output.status.success() {
            return Err(format!(
                "git worktree add failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            )
            .into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn templates_parse_and_reject_unknown_fields() {
        let templates = parse(
            "pay:\n  repo: ~/git/pay\n  base: origin/main\n  boss: claude:fable\n  workers: [claude, codex:gpt-6.1-sol]\nsolo:\n  repo: /r\n  workers: []\n",
        )
        .unwrap();
        assert_eq!(templates["pay"].base.as_deref(), Some("origin/main"));
        assert_eq!(templates["pay"].workers.as_ref().unwrap().len(), 2);
        assert_eq!(templates["solo"].workers, Some(vec![]));
        assert_eq!(templates["solo"].boss, None);
        assert!(parse("x:\n  repo: /r\n  wrokers: []\n").is_err());
    }

    #[test]
    fn branch_names_become_task_names() {
        assert_eq!(slug("feat/xyz"), "feat-xyz");
        assert_eq!(slug("-x/y z"), "x-y-z");
    }
}
