"""Test mixed launches without starting agent processes or a herdr server."""

import fcntl
import hashlib
import errno
import json
import os
import pty
import select
import struct
from pathlib import Path
import shlex
import subprocess
import sys
import tempfile
import termios
import time
import unittest
from support import copy_package


ROOT = Path(__file__).resolve().parent.parent
LAUNCHER = ROOT / "bin/herdr-shepherd"


class LauncherTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name).resolve()
        self.bin = self.root / "bin"
        self.bin.mkdir()
        self.log = self.root / "calls.jsonl"
        self.agents = self.root / "agents.json"
        self.panes = self.root / "panes.json"
        self.agent_log = self.root / "agent.json"
        self.agents.write_text("[]")
        self.panes.write_text("{}")
        self.env = dict(os.environ, PATH=f"{self.bin}:{os.environ['PATH']}", HERD_TEST_LOG=str(self.log),
                        HERD_TEST_AGENTS=str(self.agents), HERD_TEST_PANES=str(self.panes),
                        HERD_AGENT_LOG=str(self.agent_log), TMPDIR=str(self.root))
        self.executable("herdr", """#!/usr/bin/env python3
import json, os, sys
from pathlib import Path
args = sys.argv[1:]
log = Path(os.environ['HERD_TEST_LOG'])
calls = log.read_text().splitlines() if log.exists() else []
with log.open('a') as stream:
    stream.write(json.dumps(args) + '\\n')
if os.environ.get('HERD_TEST_OFFLINE'):
    print('socket unavailable', file=sys.stderr)
    sys.exit(1)
if os.environ.get('HERD_TEST_ERROR') == ' '.join(args[:2]):
    print(json.dumps({'error': {'message': 'requested operation failed'}}))
    sys.exit(0)
if os.environ.get('HERD_TEST_EMPTY') == ' '.join(args[:2]):
    sys.exit(0)
if args[:2] == ['tab', 'create']:
    result = {'root_pane': {'pane_id': 'boss-pane'}}
elif args[:2] == ['pane', 'split']:
    result = {'pane': {'pane_id': f'worker-{len(calls)}'}}
elif args[:2] == ['agent', 'list']:
    result = {'agents': json.loads(Path(os.environ['HERD_TEST_AGENTS']).read_text())}
elif args[:2] == ['pane', 'get']:
    panes = json.loads(Path(os.environ['HERD_TEST_PANES']).read_text())
    if args[2] not in panes:
        print(json.dumps({'error': {'message': 'pane not found'}}))
        sys.exit(0)
    result = {'pane': panes[args[2]]}
elif args[:2] == ['agent', 'rename']:
    if os.environ.get('HERD_TEST_RENAME_ERROR'):
        print(json.dumps({'error': {'message': 'rename failed'}}))
        sys.exit(0)
    path = Path(os.environ['HERD_TEST_AGENTS'])
    agents = json.loads(path.read_text())
    current = next((agent for agent in agents if agent['pane_id'] == args[2]), None)
    if current is None:
        current = {'pane_id': args[2]}
        agents.append(current)
    current['name'] = args[3]
    path.write_text(json.dumps(agents))
    result = {}
elif args[:2] == ['tab', 'list']:
    result = {'tabs': []}
else:
    result = {}
if os.environ.get('HERD_TEST_INVALID_PANE') == ' '.join(args[:2]):
    for value in result.values():
        if isinstance(value, dict) and 'pane_id' in value:
            value['pane_id'] = ''
print(json.dumps({'result': result}))
""")
        for name in ["claude", "codex"]:
            self.executable(name, """#!/usr/bin/env python3
import json, os, sys
from pathlib import Path
Path(os.environ['HERD_AGENT_LOG']).write_text(json.dumps(sys.argv))
sys.exit(int(os.environ.get('HERD_AGENT_EXIT', '0')))
""")
        self.executable("sleep", "#!/bin/sh\nexit 0\n")

    def executable(self, name, text):
        path = self.bin / name
        path.write_text(text)
        path.chmod(0o755)

    def run_launcher(self, *args, launcher=LAUNCHER, input=None):
        return subprocess.run([str(launcher), *args], env=self.env,
                              cwd=self.root, text=True, capture_output=True, input=input, timeout=10)

    def test_interactive_defaults_preview_without_external_commands(self):
        self.env["PATH"] = str(self.root / "empty-path")
        result = self.run_launcher("--interactive", "--dry-run", input="\n" * 5)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("Command: herdr-shepherd my-task", result.stdout)
        self.assertIn("--boss-agent claude --worker-agent claude --workers 3", result.stdout)
        self.assertIn("--model fable", result.stdout)
        self.assertIn("--model opus", result.stdout)
        self.assertEqual(self.calls(), [])

    def test_interactive_choices_use_the_existing_launch_path(self):
        folder = self.root / "project 'with' spaces"
        folder.mkdir()
        result = self.run_launcher("--interactive", input=f"interactive-test\n{folder}\n2\nclaude\n1\ny\n")
        self.assertEqual(result.returncode, 0, result.stderr)
        launches = self.launches()
        self.assertEqual([args[0] for args in launches], ["codex", "claude"])
        self.assertEqual([args[args.index("--model") + 1] for args in launches], ["gpt-6-astra", "opus"])
        for call in self.calls():
            if "--cwd" in call:
                self.assertEqual(call[call.index("--cwd") + 1], str(folder))

    def test_interactive_expands_home_and_allows_zero_workers(self):
        folder = self.root / "project"
        folder.mkdir()
        self.env["HOME"] = str(self.root)
        result = self.run_launcher("--interactive", "--dry-run", input="\n~/project\ncodex\n2\n0\n")
        self.assertEqual(result.returncode, 0, result.stderr)
        command = next(line.removeprefix("Command: ") for line in result.stdout.splitlines()
                       if line.startswith("Command: "))
        args = shlex.split(command)
        self.assertEqual(args[args.index("--cwd") + 1], str(folder))
        self.assertEqual(args[args.index("--workers") + 1], "0")
        self.assertEqual(result.stdout.count("pane send-text"), 1)
        self.assertEqual(self.calls(), [])

    def test_interactive_help_does_not_prompt(self):
        result = self.run_launcher("--interactive", "-h", input="")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("--interactive [--dry-run]", result.stdout)
        self.assertNotIn("Task name [", result.stdout)
        self.assertEqual(self.calls(), [])

    def test_interactive_cancel_eof_and_conflicting_options_create_no_panes(self):
        for args, answer, expected in [
            (["--interactive"], "\n" * 5 + "n\n", 0),
            (["--interactive"], "", 2),
            (["--interactive"], "task\n", 2),
            (["--interactive", "--team"], "", 2),
            (["task", "--interactive"], "", 2),
        ]:
            with self.subTest(args=args, answer=answer):
                result = self.run_launcher(*args, input=answer)
                self.assertEqual(result.returncode, expected, result.stderr)
                self.assertEqual(self.calls(), [])

    def run_terminal(self, *args, keys=b"\x1b", size=(28, 100)):
        master, slave = pty.openpty()
        fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", *size, 0, 0))
        original = termios.tcgetattr(slave)
        original[-1] = [value[0] if isinstance(value, bytes) else value for value in original[-1]]
        process = None
        try:
            wrapper = """import json, subprocess, sys, termios
result = subprocess.run(sys.argv[1:])
settings = termios.tcgetattr(0)
settings[-1] = [value[0] if isinstance(value, bytes) else value for value in settings[-1]]
print('\\nHERD_TERMIOS=' + json.dumps(settings), flush=True)
sys.stdin.readline()
sys.exit(result.returncode)
"""
            process = subprocess.Popen([sys.executable, "-c", wrapper, str(LAUNCHER), *args], cwd=self.root,
                                       env=dict(self.env, TERM="xterm-256color"),
                                       stdin=slave, stdout=slave, stderr=slave,
                                       start_new_session=True,
                                       preexec_fn=lambda: fcntl.ioctl(0, termios.TIOCSCTTY, 0))
            output = bytearray()
            deadline = time.monotonic() + 10

            def read_output():
                while select.select([master], [], [], 0.1)[0]:
                    try:
                        chunk = os.read(master, 8192)
                    except OSError as error:
                        if error.errno == errno.EIO:
                            break
                        raise
                    if not chunk:
                        break
                    output.extend(chunk)

            while b"\x1b[?2004h" not in output:
                read_output()
                self.assertIsNone(process.poll(), output.decode(errors="replace"))
                self.assertLess(time.monotonic(), deadline, "The form did not appear. " + repr(bytes(output)))
            os.write(master, keys)
            restored = False
            while process.poll() is None:
                read_output()
                if not restored and b"HERD_TERMIOS=" in output:
                    settings = bytes(output).split(b"HERD_TERMIOS=", 1)[1].splitlines()[0]
                    self.assertEqual(json.loads(settings), original, "The terminal settings were not restored.")
                    restored = True
                    os.write(master, b"\n")
                self.assertLess(time.monotonic(), deadline, "The terminal setup did not exit. " + output.decode(errors="replace")[-4000:])
            read_output()
            self.assertTrue(restored)
            return process.returncode, output.decode(errors="replace")
        finally:
            if process is not None and process.poll() is None:
                os.killpg(process.pid, 9)
                process.wait()
            os.close(master)
            os.close(slave)

    def test_no_arguments_open_ratatui_and_cancel_restores_the_terminal(self):
        for key in (b"\x1b", b"\x03"):
            with self.subTest(key=key):
                status, output = self.run_terminal(keys=key)
                self.assertEqual(status, 0, output)
                self.assertIn("Task", output)
                self.assertIn("Setup cancelled.", output)
                self.assertIn("\x1b[?1049l", output)
                self.assertEqual(self.calls(), [])

    def test_ratatui_preview_uses_choices_and_restores_the_terminal(self):
        keys = b"tui-team\r\r\x1b[C\r\x1b[C\r0\r\r"
        status, output = self.run_terminal("--interactive", "--dry-run", keys=keys)
        self.assertEqual(status, 0, output)
        self.assertIn("gpt-6-astra", output)
        self.assertIn("tui-team-boss", output)
        self.assertIn("\x1b[?1049l", output)
        self.assertEqual(self.calls(), [])

    def test_ratatui_restores_the_terminal_before_a_launch_error(self):
        self.env["HERD_TEST_ERROR"] = "tab create"
        status, output = self.run_terminal(keys=b"\r\r\r\r0\r\r")
        self.assertEqual(status, 2, output)
        self.assertIn("requested operation failed", output)
        self.assertIn("\x1b[?1049l", output)

    def test_small_terminal_can_cancel_without_creating_a_team(self):
        status, output = self.run_terminal(size=(10, 40), keys=b"\r\r\x1b")
        self.assertEqual(status, 0, output)
        self.assertIn("Resize", output)
        self.assertEqual(self.calls(), [])

    def calls(self):
        return [json.loads(line) for line in self.log.read_text().splitlines()] if self.log.exists() else []

    def launches(self):
        return [shlex.split(call[3]) for call in self.calls() if call[:2] == ["pane", "send-text"]]

    def team(self, current_agent=None, current_name=None, workers=(1, 3)):
        self.env["HERDR_PANE_ID"] = "current-pane"
        common = {"cwd": str(self.root), "agent_status": "idle", "tab_id": "boss-tab"}
        agents = [dict(common, pane_id="boss-pane", name="test-boss", agent="codex")]
        agents.extend(dict(common, pane_id=f"pane-{number}", name=f"test-w{number}", agent="claude")
                      for number in workers)
        current = dict(common, pane_id="current-pane", agent=current_agent, tab_id="other-tab")
        if current_name:
            current["name"] = current_name
        if current_agent or current_name:
            agents.append(current)
        self.agents.write_text(json.dumps(agents))
        self.panes.write_text(json.dumps({"current-pane": current}))

    def mutations(self):
        return [call for call in self.calls() if call[:2] not in
                [["agent", "list"], ["pane", "get"], ["tab", "list"]]]

    def test_default_launch_preserves_claude_models_and_names(self):
        result = self.run_launcher("test")
        self.assertEqual(result.returncode, 0, result.stderr)
        launches = self.launches()
        self.assertEqual([argv[0] for argv in launches], ["claude"] * 4)
        self.assertEqual([argv[4] for argv in launches], ["fable", "opus", "opus", "opus"])
        names = ["test-boss", "test-w1", "test-w2", "test-w3"]
        self.assertEqual([argv[2] for argv in launches], names)
        self.assertEqual([call[3] for call in self.calls() if call[:2] == ["agent", "rename"]], names)
        for argv in launches:
            self.assertIn("Read the herdr-shepherd skill at", argv[-1])

    def test_each_boss_type_supports_mixed_workers(self):
        for boss in ["claude", "codex"]:
            with self.subTest(boss=boss):
                self.log.unlink(missing_ok=True)
                result = self.run_launcher("mixed", "--boss-agent", boss,
                                           "--worker-agent", "codex", "--worker-agent", "claude")
                self.assertEqual(result.returncode, 0, result.stderr)
                launches = self.launches()
                self.assertEqual([argv[0] for argv in launches], [boss, "codex", "claude"])
                for role, argv in enumerate(launches):
                    if argv[0] == "codex":
                        expected_model = "gpt-6-astra" if role == 0 else "gpt-6.1-sol"
                        self.assertIn("--model", argv)
                        self.assertEqual(argv[argv.index("--model") + 1], expected_model)
                        self.assertNotIn("--name", argv)
                        self.assertIn("--approve-for-me", argv)
                self.assertIn("Your boss is mixed-boss.", launches[1][-1])
                self.assertEqual(sum(call[:2] == ["pane", "send-keys"] for call in self.calls()), 3)

    def test_single_worker_type_applies_to_the_worker_count(self):
        result = self.run_launcher("test", "--boss-agent", "codex", "--worker-agent", "codex", "--workers", "02")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual([argv[0] for argv in self.launches()], ["codex"] * 3)
        self.assertEqual([argv[argv.index("--model") + 1] for argv in self.launches()],
                         ["gpt-6-astra", "gpt-6.1-sol", "gpt-6.1-sol"])

    def test_codex_model_options_override_role_defaults(self):
        for options, expected in [
            (["--boss-model", "boss-model", "--worker-model", "worker-model"], ["boss-model", "worker-model"]),
            (["--model", "shared-model"], ["shared-model", "shared-model"]),
        ]:
            with self.subTest(options=options):
                self.log.unlink(missing_ok=True)
                result = self.run_launcher("test", "--boss-agent", "codex", "--worker-agent", "codex",
                                           "--workers", "1", *options)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual([argv[argv.index("--model") + 1] for argv in self.launches()], expected)

    def test_zero_workers_and_explicit_models(self):
        result = self.run_launcher("test", "--boss-agent", "codex", "--workers", "0", "--boss-model", "chosen-model")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(self.launches()[0][:-1], ["codex", "--approve-for-me", "--model", "chosen-model"])
        self.assertEqual(len(self.launches()), 1)
        self.assertFalse(any(call[:2] == ["pane", "split"] for call in self.calls()))

    def test_model_and_initial_prompt_remain_single_arguments(self):
        model = "custom 'model'; touch SHOULD_NOT_EXIST; $(echo unsafe)"
        result = self.run_launcher("test", "--workers", "1", "--model", model)
        self.assertEqual(result.returncode, 0, result.stderr)
        for argv in self.launches():
            self.assertEqual(argv[argv.index("--model") + 1], model)
            self.assertEqual(len(argv), 6)
        self.assertFalse((self.root / "SHOULD_NOT_EXIST").exists())

    def test_invalid_arguments_create_no_panes(self):
        cases = [[], ["test", "--boss-agent", "other"], ["test", "--worker-agent", "other"],
                 ["test", "--workers", "-1"], ["test", "--workers", "text"],
                 ["test", "--workers", "10000"],
                 ["test", "--cwd", str(self.root / "missing")], ["test", "--unknown"],
                 ["test", "--boss-model"], ["test", "--cwd", "--dry-run"],
                 ["test; echo unsafe"], ["test", "another-task"],
                 ["test", "--workers", "3", "--worker-agent", "claude", "--worker-agent", "codex"]]
        for args in cases:
            with self.subTest(args=args):
                result = self.run_launcher(*args)
                self.assertEqual(result.returncode, 2, result.stderr)
                self.assertEqual(self.calls(), [])

    def test_dry_run_matches_real_commands_and_calls_no_programs(self):
        args = ["test", "--boss-agent", "codex", "--worker-agent", "claude", "--worker-agent", "codex"]
        dry = self.run_launcher(*args, "--dry-run")
        self.assertEqual(dry.returncode, 0, dry.stderr)
        self.assertEqual(self.calls(), [])
        planned = [shlex.split(line)[1:] for line in dry.stdout.splitlines() if line.startswith("  herdr ")]
        real = self.run_launcher(*args)
        self.assertEqual(real.returncode, 0, real.stderr)
        calls = self.calls()
        mapping = {"boss-pane": "<boss>"}
        panes = [call[2] for call in calls if call[:2] == ["agent", "rename"]][1:]
        mapping.update({pane: f"<w{i}>" for i, pane in enumerate(panes, 1)})
        actual = [[mapping.get(arg, arg) for arg in call] for call in calls if call[:2] != ["pane", "focus"]]
        self.assertEqual(planned, actual)

    def test_symlink_launch_resolves_the_skill(self):
        link = self.bin / "herd-boss"
        link.symlink_to(LAUNCHER)
        result = self.run_launcher("test", "--workers", "0", launcher=link)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn(str(ROOT / "skills/herdr-shepherd/SKILL.md"), self.launches()[0][-1])

    def test_missing_selected_binary_stops_before_tab_creation(self):
        (self.bin / "codex").unlink()
        self.env["PATH"] = f"{self.bin}:/usr/bin:/bin"
        result = self.run_launcher("test", "--boss-agent", "codex")
        self.assertEqual(result.returncode, 2, result.stderr)
        self.assertIn("codex is not on PATH", result.stderr)
        self.assertEqual(self.calls(), [])

    def test_dry_run_needs_no_agent_or_herdr_binary(self):
        for name in ["herdr", "claude", "codex"]:
            (self.bin / name).unlink()
        self.env["PATH"] = str(self.root / "empty-path")
        result = self.run_launcher("test", "--boss-agent", "codex", "--dry-run")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(self.calls(), [])

    def test_add_worker_starts_selected_agent_in_the_current_terminal(self):
        self.team()
        self.env["HERD_AGENT_EXIT"] = "7"
        result = self.run_launcher("test", "--add-worker", "--worker-agent", "codex", "--worker-model", "chosen-model")
        self.assertEqual(result.returncode, 7, result.stderr)
        argv = json.loads(self.agent_log.read_text())
        self.assertEqual(Path(argv[0]).name, "codex")
        self.assertEqual(argv[1:-1], ["--approve-for-me", "--model", "chosen-model"])
        self.assertIn("You are test-w4.", argv[-1])
        self.assertIn("Your boss is test-boss.", argv[-1])
        self.assertEqual(self.mutations(), [["agent", "rename", "current-pane", "test-w4"],
                                            ["pane", "rename", "current-pane", "test-w4"]])

    def test_add_worker_starts_claude_by_default(self):
        self.team(workers=())
        result = self.run_launcher("test", "--add-worker")
        self.assertEqual(result.returncode, 0, result.stderr)
        argv = json.loads(self.agent_log.read_text())
        self.assertEqual([Path(argv[0]).name, *argv[1:-1]], ["claude", "--name", "test-w1", "--model", "opus"])

    def test_add_codex_worker_uses_the_worker_default_model(self):
        self.team()
        result = self.run_launcher("test", "--add-worker", "--worker-agent", "codex")
        self.assertEqual(result.returncode, 0, result.stderr)
        argv = json.loads(self.agent_log.read_text())
        self.assertEqual(argv[1:-1], ["--approve-for-me", "--model", "gpt-6.1-sol"])

    def test_add_worker_preserves_each_running_agent_and_registration(self):
        for kind in ["claude", "codex"]:
            with self.subTest(agent=kind):
                self.team(current_agent=kind)
                for unused in range(2):
                    result = self.run_launcher("test", "--add-worker")
                    self.assertEqual(result.returncode, 0, result.stderr)
                    self.assertIn("test-w4", result.stdout)
                    self.assertFalse(self.agent_log.exists())
                names = [agent.get("name") for agent in json.loads(self.agents.read_text())]
                self.assertEqual(names.count("test-w4"), 1)
                self.assertNotIn("test-w5", names)

    def test_add_worker_dry_run_does_not_register_or_start_an_agent(self):
        self.team()
        result = self.run_launcher("test", "--add-worker", "--worker-agent", "codex", "--dry-run")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("test-w4", result.stdout)
        self.assertIn("--approve-for-me", result.stdout)
        self.assertIn("--model gpt-6.1-sol", result.stdout)
        self.assertEqual(self.mutations(), [])
        self.assertFalse(self.agent_log.exists())

    def test_add_worker_rejects_unsafe_targets_before_changes(self):
        cases = [
            {"name": "outside-herdr", "remove_pane": True},
            {"name": "stale-pane", "pane": "missing"},
            {"name": "boss-pane", "pane": "boss-pane"},
            {"name": "other-team", "current_name": "other-w1"},
            {"name": "missing-boss", "bosses": 0},
            {"name": "duplicate-boss", "bosses": 2},
            {"name": "duplicate-worker", "current_agent": "codex", "current_name": "test-w3"},
            {"name": "other-agent", "current_agent": "opencode"},
            {"name": "change-agent", "current_agent": "codex", "args": ["--worker-agent", "claude"]},
            {"name": "change-model", "current_agent": "codex", "args": ["--worker-model", "chosen-model"]},
            {"name": "change-directory", "current_agent": "claude", "args": ["--cwd", str(self.root)]},
        ]
        for case in cases:
            with self.subTest(case=case["name"]):
                self.log.unlink(missing_ok=True)
                self.team(current_agent=case.get("current_agent"), current_name=case.get("current_name"))
                if case.get("remove_pane"):
                    self.env.pop("HERDR_PANE_ID")
                if "pane" in case:
                    self.env["HERDR_PANE_ID"] = case["pane"]
                if "bosses" in case:
                    agents = json.loads(self.agents.read_text())
                    self.agents.write_text(json.dumps(agents[1:] if case["bosses"] == 0 else agents + [agents[0]]))
                result = self.run_launcher("test", "--add-worker", *case.get("args", []))
                self.assertEqual(result.returncode, 2, result.stderr)
                self.assertEqual(self.mutations(), [])
                self.assertFalse(self.agent_log.exists())

    def test_add_worker_reports_json_errors_even_when_herdr_exits_zero(self):
        self.team()
        self.env["HERD_TEST_RENAME_ERROR"] = "1"
        result = self.run_launcher("test", "--add-worker")
        self.assertEqual(result.returncode, 2, result.stderr)
        self.assertIn("rename failed", result.stderr)
        self.assertFalse(self.agent_log.exists())

    def test_team_output_includes_workers_from_other_tabs(self):
        self.team(current_agent="codex", current_name="test-w4")
        for args in [["--team"], ["--team", "--json"]]:
            with self.subTest(args=args):
                result = self.run_launcher("test", *args)
                self.assertEqual(result.returncode, 0, result.stderr)
                if "--json" in args:
                    members = json.loads(result.stdout)[0]["members"]
                    self.assertEqual([member["name"] for member in members], ["test-boss", "test-w1", "test-w3", "test-w4"])
                else:
                    self.assertIn("boss", result.stdout)
                    self.assertIn("w4", result.stdout)
        self.assertEqual(self.mutations(), [])

    def test_modes_reject_incompatible_options(self):
        for args in [["--json"], ["--team", "--add-worker"], ["--team", "--dry-run"],
                     ["--team", "--worker-model", "model"], ["--add-worker", "--workers", "1"],
                     ["--add-worker", "--worker-agent", "claude", "--worker-agent", "codex"]]:
            with self.subTest(args=args):
                result = self.run_launcher("test", *args)
                self.assertEqual(result.returncode, 2, result.stderr)
                self.assertEqual(self.calls(), [])

    def test_simultaneous_joins_allocate_distinct_worker_numbers(self):
        self.team()
        panes = {f"join-{number}": {"pane_id": f"join-{number}"} for number in range(3)}
        self.panes.write_text(json.dumps(panes))
        processes = [subprocess.Popen([str(LAUNCHER), "test", "--add-worker"], cwd=self.root,
                                      env=dict(self.env, HERDR_PANE_ID=pane), stdout=subprocess.PIPE,
                                      stderr=subprocess.PIPE, text=True) for pane in panes]
        for process in processes:
            stdout, stderr = process.communicate(timeout=10)
            self.assertEqual(process.returncode, 0, stderr)
        names = {agent.get("name") for agent in json.loads(self.agents.read_text())}
        self.assertTrue({"test-w4", "test-w5", "test-w6"}.issubset(names))

    def registration_lock_path(self):
        key = self.env.get("HERDR_SOCKET_PATH", "") + "\0test"
        digest = hashlib.sha256(key.encode()).hexdigest()
        return self.root / f"herd-join-{os.getuid()}-{digest}.lock"

    def test_join_waits_for_the_previous_python_launcher_lock(self):
        self.team(current_agent="codex")
        with self.registration_lock_path().open("w") as lock:
            fcntl.flock(lock, fcntl.LOCK_EX)
            with subprocess.Popen([str(LAUNCHER), "test", "--add-worker"], cwd=self.root,
                                  env=self.env, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                                  text=True) as process:
                try:
                    time.sleep(0.2)
                    status = process.poll()
                    calls = self.calls()
                finally:
                    fcntl.flock(lock, fcntl.LOCK_UN)
                stdout, stderr = process.communicate(timeout=10)
                self.assertIsNone(status)
                self.assertEqual(calls, [])
                self.assertEqual(process.returncode, 0, stderr)
                self.assertIn("test-w4", stdout)

    def test_join_rejects_a_symlink_at_the_lock_path(self):
        self.team()
        target = self.root / "preserved-file"
        target.write_text("keep this file")
        self.registration_lock_path().symlink_to(target)
        result = self.run_launcher("test", "--add-worker")
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(self.calls(), [])
        self.assertEqual(target.read_text(), "keep this file")

    def test_relocated_package_runs_without_personal_configuration(self):
        checkout = self.root / "standalone checkout"
        copy_package(ROOT, checkout)
        (self.bin / "python3").symlink_to(sys.executable)
        self.env = {key: value for key, value in self.env.items() if key.startswith("HERD_TEST_")}
        self.env.update(PATH=f"{self.bin}:/usr/bin:/bin", HOME=str(self.root / "empty-home"))
        result = self.run_launcher("test", "--boss-agent", "codex", "--worker-agent", "claude",
                                   "--workers", "1", "--cwd", str(self.root),
                                   launcher=checkout / "bin/herdr-shepherd")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(len(self.launches()), 2)
        for argv in self.launches():
            self.assertIn(str(checkout / "skills/herdr-shepherd/SKILL.md"), argv[-1])
            self.assertNotIn(str(ROOT), argv[-1])

    def test_legacy_command_and_version(self):
        result = self.run_launcher("test", "--workers", "0", launcher=ROOT / "bin/herd-boss")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(len(self.launches()), 1)
        self.log.unlink()
        result = self.run_launcher("--version")
        self.assertEqual(result.stdout.strip(), "herdr-shepherd 0.1.0")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(self.calls(), [])

    def test_add_worker_rejects_folder_mismatches_before_registration(self):
        other = self.root / "other"
        other.mkdir()
        for running in (False, True):
            with self.subTest(running=running):
                self.log.unlink(missing_ok=True)
                self.team(current_agent="codex" if running else None)
                if running:
                    panes = json.loads(self.panes.read_text())
                    panes["current-pane"]["cwd"] = str(other)
                    self.panes.write_text(json.dumps(panes))
                    options = []
                else:
                    options = ["--cwd", str(other)]
                result = self.run_launcher("test", "--add-worker", *options)
                self.assertEqual(result.returncode, 2, result.stderr)
                self.assertIn("boss folder", result.stderr)
                self.assertEqual(self.mutations(), [])
                self.assertFalse(self.agent_log.exists())

    def test_server_errors_stop_launch_even_when_herdr_exits_zero(self):
        for operation in ("tab create", "pane rename", "agent rename", "pane send-text"):
            with self.subTest(operation=operation):
                self.log.unlink(missing_ok=True)
                self.env["HERD_TEST_ERROR"] = operation
                result = self.run_launcher("test", "--workers", "0")
                self.assertNotEqual(result.returncode, 0)
                self.assertIn("requested operation failed", result.stderr)
                self.assertFalse(any(call[:2] == ["pane", "send-keys"] for call in self.calls()))

    def test_empty_pane_ids_stop_launch_before_targeting_another_pane(self):
        for operation in ("tab create", "pane split"):
            with self.subTest(operation=operation):
                self.log.unlink(missing_ok=True)
                self.env["HERD_TEST_INVALID_PANE"] = operation
                result = self.run_launcher("test", "--workers", "1")
                self.assertNotEqual(result.returncode, 0)
                self.assertIn("pane_id", result.stderr)
                self.assertFalse(any("" in call for call in self.calls()))

    def test_team_list_excludes_names_without_a_team_role(self):
        self.team()
        agents = json.loads(self.agents.read_text())
        agents.append(dict(agents[0], name="test-helper", pane_id="unrelated-pane"))
        self.agents.write_text(json.dumps(agents))
        result = self.run_launcher("test", "--team", "--json")
        self.assertEqual(result.returncode, 0, result.stderr)
        members = json.loads(result.stdout)[0]["members"]
        self.assertEqual([member["name"] for member in members], ["test-boss", "test-w1", "test-w3"])
        self.assertTrue(all(member["cwd"] == str(self.root) for member in members))

    def test_team_list_reports_server_errors(self):
        self.env["HERD_TEST_ERROR"] = "agent list"
        result = self.run_launcher("test", "--team", "--json")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("requested operation failed", result.stderr)
        self.assertEqual(result.stdout, "")

    def test_successful_mutations_can_return_no_output(self):
        for operation in ("pane send-text", "pane send-keys"):
            with self.subTest(operation=operation):
                self.log.unlink(missing_ok=True)
                self.env["HERD_TEST_EMPTY"] = operation
                result = self.run_launcher("test", "--workers", "0")
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertTrue(any(call[:2] == ["pane", "send-keys"] for call in self.calls()))

    def test_viewer_uses_agent_type_for_names_without_a_team_role(self):
        self.team()
        agents = json.loads(self.agents.read_text())
        agents.append(dict(agents[0], name="api-review", pane_id="unrelated-pane"))
        self.agents.write_text(json.dumps(agents))
        result = subprocess.run([str(ROOT / "bin/herd-view"), "--json"], env=self.env,
                                text=True, capture_output=True, timeout=10)
        self.assertEqual(result.returncode, 0, result.stderr)
        member = next(member for group in json.loads(result.stdout) for member in group["members"]
                      if member["name"] == "api-review")
        self.assertEqual(member["role"], "codex")

    def test_team_list_explains_unreachable_server(self):
        self.env["HERD_TEST_OFFLINE"] = "1"
        result = self.run_launcher("test", "--team")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("Check that the herdr server is running", result.stderr)
        self.assertIn("socket unavailable", result.stderr)
        self.assertEqual(result.stdout, "")


if __name__ == "__main__":
    unittest.main()
