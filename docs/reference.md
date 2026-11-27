# Command reference

[Back to the quick start](../README.md).

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
The text prompts accept the agent entries `claude`, `codex`, `pi`, `opencode`, `1` through `4`, and `AGENT:MODEL`, and a comma-separated worker list.
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

herdr-shepherd opens the team tab in the focused herdr workspace.
Use `--workspace ID` to open it in another workspace.
Run `herdr workspace list` to find the workspace ID.
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
| pi | its configured default | its configured default |
| opencode | the `model` key in the opencode config | the `model` key in the opencode config |

Use `--boss-model` or `--worker-model` to change the model for that role.
Use `--model` to change both roles.
A worker model override applies to every worker type.
A model in an agent entry, such as `codex:gpt-6.1-sol`, overrides these options for that agent.
The agent command rejects a model that it does not support.
Select models that your account and each selected agent support.

Codex starts with `--approve-for-me`.
This retains the workspace sandbox and uses automatic approval review.
opencode starts with `--auto`, which approves each permission that the configuration does not deny.
opencode 2 sends the first prompt through its shared background service, which uses the `model` key in `~/.config/opencode/opencode.json`.
Without that key, opencode selects a default from its configured providers, and that default can be a provider that you cannot use.
Set the key, for example `"model": "yld/glm-5.3-flash"`, and run `opencode service restart`.
A team cannot give different opencode agents different models.
Shepherd rejects a model for opencode, from an agent entry or from `--model`, `--boss-model` or `--worker-model`.
pi starts with `--name` and the session name.
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
From an existing Claude Code, Codex, pi or opencode session, use `--add-worker` without model or folder options.
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

A tag `vX.Y.Z` that matches the `version` in `Cargo.toml` builds four packages and publishes a release:

```sh
git tag vX.Y.Z
git push origin vX.Y.Z
```

## License

[MIT](../LICENSE).
