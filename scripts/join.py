"""Register the current herdr pane with an existing team."""

import fcntl
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile


def call(*args):
    response = subprocess.run(["herdr", *args], capture_output=True, text=True, check=True, timeout=10)
    data = json.loads(response.stdout)
    if "error" in data:
        raise ValueError(data["error"]["message"])
    return data["result"]


def join(task, dry, requested, model, change_directory, cwd):
    pane = os.environ.get("HERDR_PANE_ID")
    if not pane:
        raise ValueError("Run --add-worker from the target herdr pane.")

    # Serialize worker numbers for simultaneous joins to this server and task.
    key = os.environ.get("HERDR_SOCKET_PATH", "") + "\0" + task
    digest = hashlib.sha256(key.encode()).hexdigest()
    path = Path(tempfile.gettempdir()) / f"herd-join-{os.getuid()}-{digest}.lock"
    descriptor = os.open(path, os.O_CREAT | os.O_RDWR | os.O_NOFOLLOW, 0o600)
    with os.fdopen(descriptor, "a") as lock:
        if os.fstat(lock.fileno()).st_uid != os.getuid():
            raise ValueError("The worker registration lock has a different owner.")
        fcntl.flock(lock, fcntl.LOCK_EX)
        agents = call("agent", "list")["agents"]
        bosses = [agent for agent in agents if agent.get("name") == f"{task}-boss"]
        if len(bosses) != 1 or bosses[0].get("agent") not in {"claude", "codex"}:
            raise ValueError(f"Expected one running boss named {task}-boss.")
        if bosses[0]["pane_id"] == pane:
            raise ValueError("The boss pane cannot join as its own worker.")
        boss_cwd = bosses[0].get("cwd")
        if not boss_cwd or Path(cwd).resolve() != Path(boss_cwd).resolve():
            raise ValueError(f"The worker folder must match the boss folder: {boss_cwd}")
        current = call("pane", "get", pane)["pane"]
        if current["pane_id"] != pane:
            raise ValueError("The current pane ID differs from HERDR_PANE_ID.")
        current_agent = current.get("agent")
        if current_agent and (not current.get("cwd") or Path(current["cwd"]).resolve() != Path(boss_cwd).resolve()):
            raise ValueError(f"The running worker must use the boss folder: {boss_cwd}")
        if current_agent and current_agent not in {"claude", "codex"}:
            raise ValueError(f"The current pane runs an unsupported agent: {current_agent}")
        if current_agent:
            if requested and requested != current_agent:
                raise ValueError(f"The current pane already runs {current_agent}.")
            if model or change_directory:
                raise ValueError("Model and directory options require a shell pane.")
        elif not dry and not shutil.which(requested or "claude"):
            raise ValueError(f"{requested or 'claude'} is not on PATH")

        current_name = next((agent.get("name") for agent in agents if agent["pane_id"] == pane), None)
        current_name = current_name or current.get("name") or ""
        worker = re.compile(re.escape(task) + r"-w([1-9][0-9]*)$")
        if worker.fullmatch(current_name):
            name = current_name
        else:
            if re.search(r"-(?:boss|w[1-9][0-9]*)$", current_name):
                raise ValueError(f"The current pane already belongs to another team: {current_name}")
            numbers = [int(match[1]) for agent in agents
                       if (match := worker.fullmatch(agent.get("name") or ""))]
            name = f"{task}-w{max(numbers, default=0) + 1}"
        if any(agent.get("name") == name and agent["pane_id"] != pane for agent in agents):
            raise ValueError(f"The worker name is not unique: {name}")
        if not dry:
            call("agent", "rename", pane, name)
            call("pane", "rename", pane, name)
        print(f"{name}\t{current_agent or '-'}")


if __name__ == "__main__":
    try:
        task, dry, requested, model, change_directory, cwd = sys.argv[1:]
        join(task, dry == "1", requested, model, change_directory == "1", cwd)
    except (OSError, ValueError, KeyError, subprocess.SubprocessError) as error:
        print(f"herdr-shepherd: {error}", file=sys.stderr)
        sys.exit(2)
