use crate::Result;
use serde_json::Value;
use std::{
    env,
    io::Read,
    os::unix::{fs::PermissionsExt, process::CommandExt},
    path::PathBuf,
    process::{Command, Output, Stdio},
    thread,
    time::{Duration, Instant},
};

pub fn available(program: &str) -> bool {
    env::split_paths(&env::var_os("PATH").unwrap_or_default()).any(|directory| {
        let path = directory.join(program);
        path.is_file()
            && path
                .metadata()
                .is_ok_and(|m| m.permissions().mode() & 0o111 != 0)
    })
}

pub fn output(command: &mut Command, timeout: Duration) -> Result<Output> {
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0)
        .spawn()?;
    let stdout = child
        .stdout
        .take()
        .ok_or("The child stdout pipe is missing.")?;
    let stderr = child
        .stderr
        .take()
        .ok_or("The child stderr pipe is missing.")?;
    let read = |mut stream: Box<dyn Read + Send>| {
        let mut bytes = Vec::new();
        stream.read_to_end(&mut bytes).map(|_| bytes)
    };
    let stdout = thread::spawn(move || read(Box::new(stdout)));
    let stderr = thread::spawn(move || read(Box::new(stderr)));
    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) if stdout.is_finished() && stderr.is_finished() => break Ok(status),
            Ok(_) if Instant::now() < deadline => thread::sleep(Duration::from_millis(10)),
            result => {
                // The process group includes children that can hold the output pipes open.
                unsafe {
                    libc::kill(-(child.id() as i32), libc::SIGKILL);
                }
                let _ = child.wait();
                break Err(match result {
                    Err(error) => error.to_string(),
                    _ => format!(
                        "The command timed out after {} seconds.",
                        timeout.as_secs_f64()
                    ),
                });
            }
        }
    };
    let stdout = stdout.join().map_err(|_| "The stdout reader failed.")??;
    let stderr = stderr.join().map_err(|_| "The stderr reader failed.")??;
    Ok(Output {
        status: status?,
        stdout,
        stderr,
    })
}

pub fn herdr(args: &[&str]) -> Result<Value> {
    let response = output(Command::new("herdr").args(args), Duration::from_secs(10))
        .map_err(|error| format!("Check that the herdr server is running. {error}"))?;
    if !response.status.success() {
        return Err(format!(
            "Check that the herdr server is running. {}",
            String::from_utf8_lossy(&response.stderr).trim()
        )
        .into());
    }
    if response.stdout.iter().all(u8::is_ascii_whitespace) {
        return Ok(Value::Null);
    }
    let data: Value = serde_json::from_slice(&response.stdout)?;
    if let Some(error) = data.get("error") {
        return Err(error
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("herdr returned an error.")
            .into());
    }
    data.get("result")
        .cloned()
        .ok_or_else(|| "The herdr response has no result.".into())
}

pub fn quote(value: &str) -> String {
    if !value.is_empty()
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_@%+=:,./-".contains(&b))
    {
        value.to_owned()
    } else {
        format!("'{}'", value.replace('\'', "'\\''"))
    }
}

pub fn shell_command(args: &[String]) -> String {
    args.iter()
        .map(|value| quote(value))
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn run(dry: bool, args: &[&str]) -> Result<()> {
    if dry {
        println!(
            "  herdr {}",
            args.iter()
                .map(|value| quote(value))
                .collect::<Vec<_>>()
                .join(" ")
        );
    } else {
        herdr(args)?;
    }
    Ok(())
}

pub fn home() -> Result<PathBuf> {
    env::var_os("HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .ok_or_else(|| "HOME is not set.".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shell_arguments_preserve_literal_text() {
        for argument in [
            "",
            "a'b",
            "$(touch never)",
            "line\nnext",
            "path with spaces",
        ] {
            let result = output(
                Command::new("/bin/sh").args(["-c", &format!("printf %s {}", quote(argument))]),
                Duration::from_secs(1),
            )
            .unwrap();
            assert_eq!(String::from_utf8(result.stdout).unwrap(), argument);
        }
    }

    #[test]
    fn timeout_stops_children_that_hold_output_pipes() {
        for script in ["sleep 10 & wait", "sleep 10 & exit 0"] {
            let start = Instant::now();
            let error = output(
                Command::new("/bin/sh").args(["-c", script]),
                Duration::from_millis(100),
            )
            .unwrap_err();
            assert!(error.to_string().contains("timed out"));
            assert!(start.elapsed() < Duration::from_secs(2));
        }
    }
}
