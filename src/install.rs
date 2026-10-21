use crate::{Result, package_root, process};
use std::{fs, os::unix::fs::symlink, path::PathBuf, process::Command, time::Duration};

fn version(text: &str) -> Option<(u64, u64, u64)> {
    for word in text.split_whitespace() {
        let mut parts = word.split('.');
        if let (Some(major), Some(minor), Some(patch)) = (parts.next(), parts.next(), parts.next())
        {
            let patch = patch.split(|c: char| !c.is_ascii_digit()).next()?;
            if let (Ok(major), Ok(minor), Ok(patch)) = (major.parse(), minor.parse(), patch.parse())
            {
                return Some((major, minor, patch));
            }
        }
    }
    None
}

pub fn run(args: &[String]) -> Result<()> {
    let check = match args {
        [] => false,
        [mode] if mode == "install" => false,
        [mode] if mode == "check" => true,
        _ => return Err("usage: herdr-shepherd --install [check]".into()),
    };
    let root = package_root()?;
    let home = process::home()?;
    let skill = root.join("skills/herdr-shepherd");
    let targets: Vec<(PathBuf, PathBuf)> = vec![
        (
            home.join(".local/bin/herdr-shepherd"),
            root.join("bin/herdr-shepherd"),
        ),
        (
            home.join(".local/bin/herd-boss"),
            root.join("bin/herdr-shepherd"),
        ),
        (
            home.join(".local/bin/herd-view"),
            root.join("bin/herd-view"),
        ),
        (home.join(".claude/skills/herdr-shepherd"), skill.clone()),
        (home.join(".agents/skills/herdr-shepherd"), skill),
    ];
    let mut errors = Vec::new();
    if !process::available("herdr") {
        errors.push("herdr is not on PATH.".to_owned());
    } else {
        let output = process::output(
            Command::new("herdr").arg("--version"),
            Duration::from_secs(10),
        )?;
        if !output.status.success()
            || version(&String::from_utf8_lossy(&output.stdout))
                .is_none_or(|value| value < (0, 7, 3))
        {
            errors.push("herdr 0.7.3 or later is required.".into());
        }
    }
    if !process::available("claude") && !process::available("codex") {
        errors.push("Install Claude Code or Codex and add its command to PATH.".into());
    }
    for (target, source) in &targets {
        if !source.exists() {
            errors.push(format!("The package file is missing: {}", source.display()));
            continue;
        }
        if target.is_symlink() && target.canonicalize().ok() == source.canonicalize().ok() {
            continue;
        }
        if fs::symlink_metadata(target).is_ok() {
            errors.push(format!(
                "Preserve the existing path. Move it before installation: {}",
                target.display()
            ));
        } else if check {
            errors.push(format!("The link is missing: {}", target.display()));
        }
    }
    if !errors.is_empty() {
        return Err(errors.join("\n").into());
    }
    if !check {
        for (target, source) in targets {
            if target.is_symlink() {
                continue;
            }
            fs::create_dir_all(target.parent().ok_or("The install path has no parent.")?)?;
            symlink(source, &target)?;
            println!("Linked {}", target.display());
        }
    }
    println!("The commands and both skill links are ready.");
    println!("If needed, add ~/.local/bin to PATH: export PATH=\"$HOME/.local/bin:$PATH\"");
    println!("Start herdr from a terminal before you create a team.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn herdr_versions_require_three_numbers() {
        assert_eq!(version("herdr 0.7.3"), Some((0, 7, 3)));
        assert_eq!(version("herdr 0.7.3-preview"), Some((0, 7, 3)));
        assert_eq!(version("herdr unknown"), None);
        assert_eq!(version("herdr 0.7"), None);
    }
}
