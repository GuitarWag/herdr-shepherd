use crate::{Result, cli, process};
use std::{
    env,
    io::{self, BufRead, IsTerminal, Write},
    path::PathBuf,
};

fn ask(
    input: &mut impl BufRead,
    output: &mut impl Write,
    label: &str,
    default: &str,
    parse: impl Fn(&str) -> Result<String>,
) -> Result<String> {
    loop {
        write!(output, "{label} [{default}]: ")?;
        output.flush()?;
        let mut line = String::new();
        if input.read_line(&mut line)? == 0 {
            return Err("Setup cancelled because the input ended. No team was created.".into());
        }
        let value = line.trim();
        match parse(if value.is_empty() { default } else { value }) {
            Ok(value) => return Ok(value),
            Err(error) => writeln!(output, "{error}")?,
        }
    }
}

pub fn selected_agent(value: &str) -> Result<String> {
    let value = match value {
        "1" => "claude",
        "2" => "codex",
        value => value,
    };
    cli::agent(value)?;
    Ok(value.to_owned())
}

pub fn folder(value: &str) -> Result<String> {
    let path = if value == "~" {
        process::home()?
    } else if let Some(relative) = value.strip_prefix("~/") {
        process::home()?.join(relative)
    } else {
        PathBuf::from(value)
    };
    if !path.is_dir() {
        return Err(format!("No such directory: {}", path.display()).into());
    }
    Ok(path
        .canonicalize()?
        .to_str()
        .ok_or("The folder path must use UTF-8.")?
        .to_owned())
}

fn setup(
    input: &mut impl BufRead,
    output: &mut impl Write,
    dry: bool,
) -> Result<Option<cli::Options>> {
    writeln!(output, "Set up a herdr-shepherd team.")?;
    writeln!(
        output,
        "Press Enter to use each default. Press Ctrl+C to cancel."
    )?;
    let task = ask(input, output, "Task name", "my-task", |value| {
        cli::task_name(value)?;
        Ok(value.to_owned())
    })?;
    let cwd = env::current_dir()?;
    let cwd = ask(
        input,
        output,
        "Working folder",
        cwd.to_str().ok_or("The folder path must use UTF-8.")?,
        folder,
    )?;
    writeln!(
        output,
        "Agents: 1 = Claude Code, 2 = Codex. Enter a number or command name."
    )?;
    let boss = ask(input, output, "Boss agent", "claude", selected_agent)?;
    let worker = ask(input, output, "Worker agent", "claude", selected_agent)?;
    let count = ask(input, output, "Worker count", "3", |value| {
        Ok(cli::worker_count(value)?.to_string())
    })?;
    let mut args = vec![
        task,
        "--cwd".into(),
        cwd,
        "--boss-agent".into(),
        boss,
        "--worker-agent".into(),
        worker,
        "--workers".into(),
        count,
    ];
    if dry {
        args.push("--dry-run".into());
    }
    let options = cli::parse(&args)?.ok_or("The team options are missing.")?;
    let mut command = vec!["herdr-shepherd".to_owned()];
    command.extend(args);
    writeln!(output, "\nCommand: {}", process::shell_command(&command))?;
    if !dry {
        let answer = ask(
            input,
            output,
            "Start this team? y/n",
            "y",
            |value| match value.to_ascii_lowercase().as_str() {
                "y" | "yes" => Ok("y".into()),
                "n" | "no" => Ok("n".into()),
                _ => Err("Enter y or n.".into()),
            },
        )?;
        if answer == "n" {
            writeln!(output, "Setup cancelled. No team was created.")?;
            return Ok(None);
        }
    }
    Ok(Some(options))
}

pub fn run(dry: bool) -> Result<()> {
    let options = if io::stdin().is_terminal()
        && io::stdout().is_terminal()
        && env::var("TERM").as_deref() != Ok("dumb")
    {
        crate::tui::setup(dry)?
    } else {
        setup(&mut io::stdin().lock(), &mut io::stdout().lock(), dry)?
    };
    if let Some(options) = options {
        cli::create(&options)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn invalid_answers_repeat_the_prompt_and_defaults_remain_available() {
        let mut input =
            Cursor::new("bad task\n\n/missing-shepherd-folder\n\nother\n2\n\n10000\n\n");
        let mut output = Vec::new();
        let options = setup(&mut input, &mut output, true).unwrap().unwrap();
        assert_eq!(options.task, "my-task");
        assert_eq!(options.boss_agent, "codex");
        assert_eq!(options.worker_agents, vec!["claude"; 3]);
        let text = String::from_utf8(output).unwrap();
        for label in ["Task name", "Working folder", "Boss agent", "Worker count"] {
            assert_eq!(text.matches(label).count(), 2);
        }
    }

    #[test]
    fn cancel_and_incomplete_input_never_return_launch_options() {
        assert!(
            setup(&mut Cursor::new("\n\n\n\n\nn\n"), &mut Vec::new(), false)
                .unwrap()
                .is_none()
        );
        assert!(setup(&mut Cursor::new("task\n"), &mut Vec::new(), false).is_err());
    }
}
