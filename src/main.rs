mod cli;
mod install;
mod interactive;
mod join;
mod mail;
mod process;
mod tui;
mod view;

use serde_json::Value;
use std::{
    env,
    io::{IsTerminal, Read},
    path::PathBuf,
};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

fn string<'a>(value: &'a Value, key: &str) -> Result<&'a str> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| format!("The herdr response has no string field: {key}").into())
}

fn optional<'a>(value: &'a Value, key: &str) -> &'a str {
    value.get(key).and_then(Value::as_str).unwrap_or("")
}

fn package_root() -> Result<PathBuf> {
    let executable = env::current_exe()?.canonicalize()?;
    for directory in executable
        .parent()
        .ok_or("The executable has no directory.")?
        .ancestors()
        .take(3)
    {
        if directory.join("skills/herdr-shepherd/SKILL.md").is_file() {
            return Ok(directory.to_owned());
        }
    }
    Err(
        "The bundled herdr-shepherd skill is missing. Keep the binary and skill in the package."
            .into(),
    )
}

fn run(args: &[String], viewer: bool) -> Result<()> {
    if viewer {
        return view::run(args);
    }
    if args.is_empty() && std::io::stdin().is_terminal() && std::io::stdout().is_terminal() {
        return interactive::run(false);
    }
    if args.iter().any(|arg| arg == "--interactive") {
        if args
            .iter()
            .any(|arg| matches!(arg.as_str(), "-h" | "--help"))
        {
            cli::parse(&["--help".into()])?;
            return Ok(());
        }
        if args
            .iter()
            .any(|arg| !matches!(arg.as_str(), "--interactive" | "--dry-run"))
        {
            return Err("usage: herdr-shepherd --interactive [--dry-run]".into());
        }
        return interactive::run(args.iter().any(|arg| arg == "--dry-run"));
    }
    if args.first().map(String::as_str) == Some("--install") {
        return install::run(&args[1..]);
    }
    if let Some(mut options) = cli::parse(args)? {
        if options.task.is_empty() {
            options.task = view::infer_task(&options.cwd)?;
            // The agent list is untrusted input; keep the name safe for paths.
            cli::task_name(&options.task)?;
        }
        match options.operation {
            cli::Operation::Team => view::show(Some(&options.task), options.json),
            cli::Operation::Join => join::run(&options),
            cli::Operation::Create => cli::create(&options),
            cli::Operation::Send => {
                let text = match &options.message {
                    Some(text) => text.clone(),
                    None if std::io::stdin().is_terminal() => {
                        return Err(
                            "Pass --message TEXT or pipe the message on standard input.".into()
                        );
                    }
                    None => {
                        let mut text = String::new();
                        std::io::stdin().read_to_string(&mut text)?;
                        text
                    }
                };
                mail::send(&options, &options.to, &text)
            }
            cli::Operation::Read => mail::read(&options, options.message_id.as_deref()),
        }
    } else {
        Ok(())
    }
}

fn main() {
    let arguments: std::result::Result<Vec<_>, _> =
        env::args_os().map(|arg| arg.into_string()).collect();
    let arguments = match arguments {
        Ok(arguments) => arguments,
        Err(_) => {
            eprintln!("herdr-shepherd: Command arguments must use UTF-8.");
            std::process::exit(2);
        }
    };
    let viewer = std::path::Path::new(&arguments[0])
        .file_name()
        .is_some_and(|name| name == "herd-view");
    let installing = arguments.get(1).map(String::as_str) == Some("--install");
    if let Err(error) = run(&arguments[1..], viewer) {
        eprintln!(
            "{}: {error}",
            if viewer {
                "herd-view"
            } else {
                "herdr-shepherd"
            }
        );
        std::process::exit(if viewer || installing { 1 } else { 2 });
    }
}
