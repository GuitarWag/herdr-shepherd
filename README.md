# herdr-shepherd

[![ci](https://github.com/GuitarWag/herdr-shepherd/actions/workflows/ci.yml/badge.svg)](https://github.com/GuitarWag/herdr-shepherd/actions/workflows/ci.yml)
[![release](https://img.shields.io/github/v/release/GuitarWag/herdr-shepherd?label=release)](https://github.com/GuitarWag/herdr-shepherd/releases/latest)
[![platforms](https://img.shields.io/badge/platforms-macOS%20%7C%20Linux-blue)](https://github.com/GuitarWag/herdr-shepherd/releases/latest)
[![rust](https://img.shields.io/badge/rust-1.89%2B-orange?logo=rust)](https://www.rust-lang.org/tools/install)
[![license](https://img.shields.io/github/license/GuitarWag/herdr-shepherd)](LICENSE)

Agent communication for Claude Code and Codex, over [herdr](https://herdr.dev).

Agents discover their teammates, send tasks, check status, and read results across separate terminal sessions.
A Codex boss can assign work to Claude Code workers.
A Claude Code boss can assign work to Codex workers.
Each agent keeps its own process, context, and permissions.

`herdr` supplies the terminal communication commands.
The shared [agent skill](skills/herdr-shepherd/SKILL.md) defines how bosses and workers use them to communicate.
The Rust CLI registers the team and helps agents find their boss or workers.
The board and its launcher help you start those sessions in a selected folder.

## How agents communicate

After you create a team, its agents can discover their teammates across tabs:

```sh
herdr-shepherd my-task --team --json
```

Check that the target is idle and contains no unrelated unsent prompt.
Replace `WORKER_PANE_ID` with the pane ID from the team output:

```sh
herdr agent send my-task-w1 "Read the tests. Report the failures without editing files."
herdr pane send-keys WORKER_PANE_ID Enter
herdr agent wait my-task-w1 --status idle --timeout 600000
herdr agent read my-task-w1 --lines 80
```

The boss sends a task and reads the worker's result.
Both `send` and `Enter` are necessary.
The timeout uses milliseconds.
Workers report in their own panes for the boss to read.
A worker can contact an idle boss with the same sequence when the boss requests a reply.
Use the [skill](skills/herdr-shepherd/SKILL.md) for the complete communication procedure.

Communication uses terminal input and output.
It does not provide a message queue, delivery acknowledgements, or a complete transcript.
An idle status alone does not prove task completion.
Read the result.
Check that it answers the task.
Team names group sessions; they do not restrict access to other sessions.

Warning: agents in the shared folder can overwrite each other's changes.
Assign separate files to editing workers, or use one editing worker at a time.

## Quick start

Install herdr 0.7.3 or later.
Authenticate Claude Code, Codex, or both.
Source builds also require [Rust](https://www.rust-lang.org/tools/install) 1.89 or later and a C linker.

From a [release](https://github.com/GuitarWag/herdr-shepherd/releases), with no Rust toolchain:

```sh
target="$(rustc -vV 2>/dev/null | sed -n 's/^host: //p')"   # or aarch64-apple-darwin, x86_64-apple-darwin, x86_64-unknown-linux-gnu, aarch64-unknown-linux-gnu
curl -fsSL "https://github.com/GuitarWag/herdr-shepherd/releases/latest/download/herdr-shepherd-$target.tar.gz" | tar -xz
./herdr-shepherd/install.sh
export PATH="$HOME/.local/bin:$PATH"
herdr-shepherd --help
```

Keep the unpacked `herdr-shepherd` folder; the installer links to it.
Unpack a newer release over the same folder to upgrade.

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

Inside herdr, open the board:

```sh
herdr-shepherd
```

## Board

Run `herdr-shepherd` without arguments in a terminal to open the board.
The board shows every director, team, and agent on the herdr server, and it refreshes every two seconds.
Teams that start with `--under` appear under their director.
The right panel shows the screen and the unread messages of the selected agent.

| Key | Action |
| --- | --- |
| Up/Down or `j`/`k` | Select a row. |
| Left/Right or `h`/`l` | Fold or unfold a group. |
| Enter | Focus the herdr tab of the selected agent. |
| `n` | Open the launcher for a new team. |
| `d` | Open the launcher for a new director. |
| `a` | Add a worker to the selected team. |
| `m` | Write a message to the selected agent. |
| `r` | Open the oldest unread message of the selected agent and mark it read. |
| `q` or Esc | Quit. |

In the message box, Enter sends the message, and Ctrl+J or Alt+Enter starts a new line.
The board sends the message with the same mailbox as `--send`, so the agent receives a one-line pointer.
Only an agent in a team has a mailbox.

`a` splits a new pane below the team's last worker, or beside the boss when the team has no workers.
The new pane runs `herdr-shepherd <task> --add-worker` with the agent type of the last worker, or Claude Code.

## Launcher

The launcher is a card over the board.
It sets the kind (team or director), task name, folder, director, boss, and workers.
The command preview at the bottom of the card changes as you edit.
When the selected row belongs to a director, a new team reports to that director.

| Key | Action |
| --- | --- |
| Up/Down or Tab | Select a field. |
| Left/Right or Space | Change a choice: kind, director, agent, model, or worker count. |
| `+` and `-` | Change the worker count. |
| Type | Edit the name, the folder, or a model. Typing in a model field enters a custom model. |
| Tab in the folder field | Complete the folder name. |
| Enter | Start the team. |
| Esc or Ctrl+C | Close the launcher. |

Each worker has its own agent and model.
A model shown in grey is the default for that role.
The launcher shows at most eight workers; use command options for a larger team.
After the team starts, the board opens again and shows `Started <task>.`

To open the launcher alone, or to preview the commands without creating panes:

```sh
herdr-shepherd --interactive
herdr-shepherd --interactive --dry-run
```

`--interactive` accepts only `--dry-run` and help options.
Piped input and terminals with `TERM=dumb` use text prompts.
The text prompts accept blank answers for defaults and `n` to cancel at confirmation.
The text prompts accept the agent entries `claude`, `codex`, `1`, `2`, and `AGENT:MODEL`, and a comma-separated worker list.
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

Start four Codex and Claude Code workers with a model for two of them:

```sh
herdr-shepherd my-task --cwd /path/to/project \
  --worker-agent codex:gpt-6.1-sol --worker-agent codex \
  --worker-agent codex --worker-agent claude:opus
```

`--boss-agent` and `--worker-agent` accept `AGENT[:MODEL]`.
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
A model in an agent entry, such as `codex:gpt-6.1-sol`, overrides these options for that agent.
The agent command rejects a model that it does not support.
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
You can omit the task name for `--add-worker` and `--team`.
The command then uses the one boss that runs in the current folder.
When no boss or more than one boss runs there, the command stops and asks for the name.
From a shell pane, `--worker-agent codex:MODEL` selects the agent and its model.
From an existing Claude Code or Codex session, use `--add-worker` without model or folder options.
The command rejects a model in `--worker-agent` from a running session.
The command registers the session and prints its worker instructions.
It preserves the running session, model, and folder.
Registration requires a valid `HERDR_PANE_ID` from the target pane.
Repeat registration keeps the same worker name.

Discover members across tabs:

```sh
herdr-shepherd --team
herdr-shepherd my-task --team --json
```

JSON includes each member's name, role, agent type, status, pane ID, and folder.
Use `herd-view --watch` to inspect all registered sessions after a full installation.

## Send messages between agents

```sh
herdr-shepherd --send w1 --message "Read src/auth.rs and list every caller."
herdr-shepherd --send boss <<'MESSAGE'
Result: ...
MESSAGE
herdr-shepherd --read m1761153600123-4242
herdr-shepherd --read
```

`--send` stores the message in `~/.local/state/herdr-shepherd/mail/<task>/<id>.md`.
It types a one-line pointer with the message ID into the target agent and submits it.
The target agent runs `--read <id>` to print the complete message.
`--read` without an ID lists the unread messages for the current pane.
Each `--send` and `--read` deletes messages older than seven days.
A new team with the same task name starts with an empty mailbox.
Claude Code drops the start of a long typed burst, so the pointer keeps every message complete.

## Direct several teams

A director manages several bosses from one pane.
It does not send messages to workers.

```sh
herdr-shepherd org --director --cwd /path/to/project
herdr-shepherd auth --cwd /path/to/project --under org --workers 2
herdr-shepherd org --send auth-boss --message "Use the existing token cache."
herdr-shepherd auth --send org-director --message "Result: ..."
```

The first command opens `org-director` in a new tab with no workers.
The director runs the second command to start a team whose boss reports to it.
Each message goes to the mailbox of the recipient's team.
A director and a team cannot share a task name.
An agent name that ends in `-director` is reserved for a director.

## Installation details

The compiled commands require herdr and the selected agent commands.
They do not require the Rust toolchain at runtime.
Git is optional; the viewer uses it to display repository names.
The package does not create or manage Git worktrees.

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
GitHub Actions runs the automated tests on macOS and Linux for every push.
Live-agent workflows are validated on macOS Apple Silicon; Linux live runs still need a report.
The tests use temporary folders and agent stubs.
The unit tests, integration tests, fake commands, and terminal supervisor all use Rust.
Run `cargo test` to check the complete suite.

```sh
cargo fmt --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked
cargo build --release --locked
```

Build the binary before running `./bin/herdr-shepherd` from a source checkout.
For a compiled package, keep the binary at `bin/herdr-shepherd` and the skill under `skills/herdr-shepherd`.
Keep the alias links beside the binary.
Build a separate binary for each operating system and processor architecture.

GitHub Actions runs fmt, clippy, and the tests on macOS and Linux for every push.
A tag `vX.Y.Z` that matches the `version` in `Cargo.toml` builds four packages and publishes a release:

```sh
git tag v0.2.0
git push origin v0.2.0
```

## License

[MIT](LICENSE).
