# herdr-shepherd

An independent tool for [herdr](https://herdr.dev).
Open Claude Code and Codex teams in a selected folder.
Let the agents discover their team, send tasks, and read results.

The package includes its CLI scripts, agent skill, installer, and tests.
It does not require Ghostty, dotfiles, another repository, or a personal skill installer.
It does not create or manage Git worktrees.

## Requirements

- macOS or Linux.
- herdr 0.7.3 or later. Version 0.7.3 is the initial supported version.
- Bash and Python 3.9 or later.
- Claude Code, Codex, or both, with authentication complete.

The default agents are Claude Code.
Claude Code uses `fable` for the boss and `opus` for workers.
Codex uses `gpt-6-astra` for the boss and `gpt-6.1-sol` for workers.
Use `--boss-model`, `--worker-model`, or `--model` when your account needs different models.
If a pane reports a model error, select an available model with the option for that role.
A worker model override applies to all worker types.
Select models that each selected agent type supports.

Codex starts with `--approve-for-me`.
This mode retains the workspace sandbox and uses automatic approval review.
The agent can request approved access to the herdr socket when its sandbox blocks communication.

## Install

Warning: starting the herdr server inside an agent can pass environment variables that break Claude sessions.
Start herdr from a normal terminal.
The installer does not start the server or download dependencies.

From this repository:

```sh
./install.sh
./install.sh check
```

The installer checks dependencies and creates these links:

| Path | Purpose |
| --- | --- |
| `~/.local/bin/herdr-shepherd` | Open, join, or inspect a team. |
| `~/.local/bin/herd-boss` | Preserve the previous command name. |
| `~/.local/bin/herd-view` | Show registered sessions. |
| `~/.claude/skills/herdr-shepherd` | Make the skill available to Claude Code. |
| `~/.agents/skills/herdr-shepherd` | Make the skill available to Codex. |

The installer preserves existing files and links.
Move conflicting paths before installation.
It does not modify your shell or terminal configuration.
If needed, add the command directory to your shell's PATH:

```sh
export PATH="$HOME/.local/bin:$PATH"
```

Keep this checkout at its installed location.
Moving or deleting it breaks the links.
Start a new agent session to discover the installed skill.
The launcher also gives every new agent the exact skill path.

You can run `./bin/herdr-shepherd` without installing links.

## Open a team

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

Every session starts in the selected folder.
The boss receives the name `my-task-boss`.
Workers receive names such as `my-task-w1`.
The startup prompt identifies each agent's role and tells it to read the skill.
The boss waits for your task.

One `--worker-agent` applies to every worker selected by `--workers N`.
Multiple entries define the worker order and count.
The default worker count is three when the entries do not define a count.
Use `--workers 0` to start only a boss.
Keep task names unique on the current herdr server.

Preview commands without creating sessions:

```sh
herdr-shepherd my-task --boss-agent codex --workers 1 --dry-run
```

## Join and discover a team

From a shell in a herdr pane:

```sh
herdr-shepherd my-task --add-worker --worker-agent codex --cwd /path/to/project
```

The command registers the current pane and starts the selected agent in that terminal.
From an existing Claude Code or Codex session, use `--add-worker` without model or folder options.
The command prints the worker instructions and preserves the existing session.
The worker must use the boss folder.
A folder mismatch stops registration before the command changes the pane name.
The command requires a valid `HERDR_PANE_ID` from the target pane.

Both roles can discover team members across tabs:

```sh
herdr-shepherd my-task --team
herdr-shepherd my-task --team --json
```

JSON includes each member's name, role, agent type, status, pane ID, and folder.
These commands still require the task name.
Use `herd-view --watch` to inspect all registered sessions.

## Communicate

Warning: agents in the shared folder can overwrite each other's changes.
Assign separate files to editing workers, or use one editing worker at a time.

Use the [skill](skills/herdr-shepherd/SKILL.md) for the complete communication procedure.
Check that the target is idle and its input contains no unrelated prompt before you send a task.
Replace `WORKER_PANE_ID` with the pane ID from the team output:

```sh
herdr agent send my-task-w1 "Read the tests. Report the failures without editing files."
herdr pane send-keys WORKER_PANE_ID Enter
herdr agent wait my-task-w1 --status idle --timeout 600000
herdr agent read my-task-w1 --lines 80
```

Workers normally report results in their own panes for the boss to read.
A worker can contact an idle boss with the same send sequence when the boss requests a reply.

Communication uses terminal input and output.
The package does not provide a message queue, delivery acknowledgements, or a complete transcript.
An idle state alone does not prove task completion.
Read the result and check that it answers the task.
Team names identify related sessions; they do not restrict access to other sessions.

## Check the package

```sh
python3 -m unittest discover -s tests -v
./bin/herdr-shepherd --help
./bin/herdr-shepherd --version
```

The tests use temporary folders and agent stubs.
They do not require an agent account or a running herdr server.
Git is optional at runtime; `herd-view` uses it only to display repository names.

The project uses the [MIT license](LICENSE).
