# herdr-shepherd

Open a team of Claude Code and Codex sessions in one folder through [herdr](https://herdr.dev).
Choose a boss and workers.
Discover their sessions.
Send tasks between them.

The package includes a Rust CLI and a shared [agent skill](skills/herdr-shepherd/SKILL.md).
Bosses and workers can use different agents and models.
The package does not create or manage Git worktrees.

## Quick start

Install herdr 0.7.3 or later.
Authenticate Claude Code, Codex, or both.
Source builds also require [Rust](https://www.rust-lang.org/tools/install) 1.89 or later and a C linker.

From a checkout of this repository:

```sh
cargo build --release --locked
./install.sh
export PATH="$HOME/.local/bin:$PATH"
herdr-shepherd --help
```

The installer makes the commands available from any folder for your user account.
It also registers the skill for Claude Code and Codex.
If an existing command blocks installation, use the [command-only installation](#keep-existing-command-aliases).

Warning: start herdr from a normal terminal outside an agent session.
A server that inherits agent environment variables can prevent Claude Code from starting.

```sh
herdr
```

Inside herdr, open the interactive setup:

```sh
herdr-shepherd
```

## Interactive setup

The [Ratatui](https://ratatui.rs) form shows the task name, working folder, agents, worker count, and command preview.
The defaults are `my-task`, your current folder, Claude Code for both roles, and three workers.

| Key | Action |
| --- | --- |
| Tab, Up, Down | Select a field. |
| Enter | Validate the field and advance. Review after the last field. |
| Left, Right | Change the selected agent. |
| Type | Replace a field's default, then append text. |
| Backspace | Delete the last character. Return from review to editing. |
| Ctrl+U | Clear the selected field. |
| Esc, Ctrl+C | Cancel setup. |

Enter `1` or `claude` for Claude Code.
Enter `2` or `codex` for Codex.
The folder accepts relative paths, absolute paths, and `~`.
The selected worker agent applies to every worker.
Press Enter on the review screen to confirm the team.
The form restores the terminal before it starts the agents.
The form requires at least 64 columns and 24 rows.

To preview the setup without creating panes:

```sh
herdr-shepherd --interactive --dry-run
```

`--interactive` accepts only `--dry-run` and help options.
Without arguments, the command opens the setup when its input and output use a terminal.
Piped input and terminals with `TERM=dumb` use text prompts.
The text prompts accept blank answers for defaults and `n` to cancel at confirmation.
Use explicit command options in scripts.

## Command options

Start a Codex boss and one Codex worker:

```sh
herdr-shepherd my-task --cwd /path/to/project \
  --boss-agent codex --worker-agent codex --workers 1
```

Start a Codex boss with one Claude Code worker and one Codex worker:

```sh
herdr-shepherd my-task --cwd /path/to/project \
  --boss-agent codex --worker-agent claude --worker-agent codex
```

One `--worker-agent` applies to the count selected by `--workers`.
Multiple entries define the worker order and count.
An explicit count must match a list with multiple entries.
The default count is three.
Use `--workers 0` to start only a boss.
Add `--dry-run` to preview commands.
Use `herdr-shepherd -h` for all options.

Every session starts in the selected folder.
The boss receives the name `my-task-boss` and waits for your task.
Workers receive names such as `my-task-w1` and wait for the boss.
Each agent receives its role and the path to the skill.
Keep task names unique on the herdr server.

## Default agents and models

Both roles default to Claude Code.

| Agent | Boss model | Worker model |
| --- | --- | --- |
| Claude Code | `fable` | `opus` |
| Codex | `gpt-6-astra` | `gpt-6.1-sol` |

Use `--boss-model` or `--worker-model` to change the model for that role.
Use `--model` to change both roles.
A worker model override applies to every worker type.
Select models that your account and each selected agent support.

Codex starts with `--approve-for-me`.
This retains the workspace sandbox and uses automatic approval review.
Socket access can require approval from the agent's permission controls.

## Join and discover a team

From a shell in a herdr pane, turn the current terminal into a worker:

```sh
herdr-shepherd my-task --add-worker --worker-agent codex --cwd /path/to/project
```

The boss must exist, and the worker must use the boss folder.
From an existing Claude Code or Codex session, use `--add-worker` without model or folder options.
The command registers the session and prints its worker instructions.
It preserves the running session, model, and folder.
Registration requires a valid `HERDR_PANE_ID` from the target pane.
Repeat registration keeps the same worker name.

Discover members across tabs:

```sh
herdr-shepherd my-task --team
herdr-shepherd my-task --team --json
```

JSON includes each member's name, role, agent type, status, pane ID, and folder.
Use `herd-view --watch` to inspect all registered sessions after a full installation.

## Send tasks and read results

Warning: agents in the shared folder can overwrite each other's changes.
Assign separate files to editing workers, or use one editing worker at a time.

Check that the target is idle and contains no unrelated unsent prompt.
Replace `WORKER_PANE_ID` with the pane ID from the team output:

```sh
herdr agent send my-task-w1 "Read the tests. Report the failures without editing files."
herdr pane send-keys WORKER_PANE_ID Enter
herdr agent wait my-task-w1 --status idle --timeout 600000
herdr agent read my-task-w1 --lines 80
```

Both `send` and `Enter` are necessary.
The timeout uses milliseconds.
Workers report in their own panes for the boss to read.
Use the [skill](skills/herdr-shepherd/SKILL.md) for the complete procedure, including replies to the boss.

Communication uses terminal input and output.
It does not provide a message queue, delivery acknowledgements, or a complete transcript.
An idle status alone does not prove task completion.
Read the result.
Check that it answers the task.
Team names group sessions; they do not restrict access to other sessions.

## Installation details

The compiled commands require herdr and the selected agent commands.
They do not require Rust, Bash, or Python at runtime.
Git is optional; the viewer uses it to display repository names.

A full installation creates these links:

| Path | Purpose |
| --- | --- |
| `~/.local/bin/herdr-shepherd` | Open, join, or inspect a team. |
| `~/.local/bin/herd-boss` | Use the previous launcher name. |
| `~/.local/bin/herd-view` | Show registered sessions. |
| `~/.claude/skills/herdr-shepherd` | Register the Claude Code skill. |
| `~/.agents/skills/herdr-shepherd` | Register the Codex skill. |

The installer preserves existing files and conflicting links.
It does not download dependencies, start herdr, or change shell configuration.
Keep the checkout at its installed location because the links point to it.
Start a new agent session to discover the installed skill.

Check a full installation or install an existing build without Cargo:

```sh
./bin/herdr-shepherd --install check
./bin/herdr-shepherd --install
```

### Keep existing command aliases

If `herd-boss` or `herd-view` already belongs to another setup, link only the new command.
From the repository folder:

```sh
mkdir -p "$HOME/.local/bin"
ln -s "$PWD/bin/herdr-shepherd" "$HOME/.local/bin/herdr-shepherd"
export PATH="$HOME/.local/bin:$PATH"
herdr-shepherd --version
```

This keeps the existing aliases and does not register global skill links.
New teams still receive the bundled skill's exact path.
The link command refuses to replace an existing command.
To keep the command available in new terminals, add the PATH line to your shell configuration if necessary.

## Development and validation

The project targets macOS and Linux.
The current Rust build passes automated tests on macOS Apple Silicon.
Linux and complete live-agent workflows still require validation.
The tests use temporary folders and agent stubs.
Python 3.9 or later is required only for the compatibility tests.

```sh
cargo fmt --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked
cargo build --release --locked
python3 -m unittest discover -s tests -v
```

Build the binary before running `./bin/herdr-shepherd` from a source checkout.
For a compiled package, keep the binary at `bin/herdr-shepherd` and the skill under `skills/herdr-shepherd`.
Keep the alias links beside the binary.
Build a separate binary for each operating system and processor architecture.

## License

[MIT](LICENSE).
