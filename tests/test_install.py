"""Test installation in temporary homes without personal configuration."""

from pathlib import Path
import shlex
import subprocess
import tempfile
import unittest
from support import copy_package


ROOT = Path(__file__).resolve().parent.parent


class InstallTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name).resolve()
        self.home = self.root / "home"
        self.home.mkdir()
        self.bin = self.root / "dependencies"
        self.bin.mkdir()
        self.command("herdr", "#!/bin/sh\necho 'herdr 0.7.3'\n")
        self.command("codex", "#!/bin/sh\nexit 0\n")
        self.env = {"HOME": str(self.home), "PATH": str(self.bin)}

    def command(self, name, content):
        path = self.bin / name
        path.write_text(content)
        path.chmod(0o755)

    def install(self, *args, checkout=ROOT):
        return subprocess.run([str(checkout / "bin/herdr-shepherd"), "--install", *args], cwd=self.home,
                              env=self.env, text=True, capture_output=True, timeout=10)

    def test_install_is_repeatable_and_registers_both_agents(self):
        for mode in ([], [], ["check"]):
            result = self.install(*mode)
            self.assertEqual(result.returncode, 0, result.stderr)
        for name in ("herdr-shepherd", "herd-boss", "herd-view"):
            self.assertTrue((self.home / ".local/bin" / name).is_symlink())
        for directory in (".claude", ".agents"):
            link = self.home / directory / "skills/herdr-shepherd"
            self.assertEqual(link.resolve(), ROOT / "skills/herdr-shepherd")
        self.assertFalse((self.home / ".zshrc").exists())
        self.assertFalse((self.home / ".config").exists())

    def test_existing_command_stops_install_before_any_new_links(self):
        target = self.home / ".local/bin/herd-boss"
        target.parent.mkdir(parents=True)
        target.write_text("keep this command")
        result = self.install()
        self.assertEqual(result.returncode, 1, result.stderr)
        self.assertIn("Preserve the existing path", result.stderr)
        self.assertEqual(target.read_text(), "keep this command")
        self.assertFalse((self.home / ".local/bin/herdr-shepherd").exists())
        self.assertFalse((self.home / ".agents").exists())

    def test_existing_skill_directory_is_preserved(self):
        target = self.home / ".agents/skills/herdr-shepherd"
        target.mkdir(parents=True)
        (target / "SKILL.md").write_text("keep this skill")
        result = self.install()
        self.assertEqual(result.returncode, 1, result.stderr)
        self.assertEqual((target / "SKILL.md").read_text(), "keep this skill")
        self.assertFalse((self.home / ".local").exists())

    def test_stale_link_is_preserved(self):
        target = self.home / ".local/bin/herdr-shepherd"
        target.parent.mkdir(parents=True)
        source = self.root / "missing-command"
        target.symlink_to(source)
        result = self.install()
        self.assertEqual(result.returncode, 1, result.stderr)
        self.assertEqual(target.readlink(), source)
        self.assertFalse((target.parent / "herd-boss").exists())

    def test_missing_agent_stops_install_before_writes(self):
        (self.bin / "codex").unlink()
        result = self.install()
        self.assertEqual(result.returncode, 1, result.stderr)
        self.assertIn("Install Claude Code or Codex", result.stderr)
        self.assertFalse((self.home / ".local").exists())

    def test_unsupported_herdr_stops_install_before_writes(self):
        self.command("herdr", "#!/bin/sh\necho 'herdr 0.7.2'\n")
        result = self.install()
        self.assertEqual(result.returncode, 1, result.stderr)
        self.assertIn("herdr 0.7.3 or later", result.stderr)
        self.assertFalse((self.home / ".local").exists())

    def test_check_mode_does_not_install_missing_links(self):
        result = self.install("check")
        self.assertEqual(result.returncode, 1, result.stderr)
        self.assertIn("The link is missing", result.stderr)
        for directory in (".local", ".agents", ".claude"):
            self.assertFalse((self.home / directory).exists())

    def test_relocated_checkout_installs_and_runs_without_original_paths(self):
        checkout = self.root / "standalone checkout"
        copy_package(ROOT, checkout)
        result = self.install(checkout=checkout)
        self.assertEqual(result.returncode, 0, result.stderr)
        launcher = self.home / ".local/bin/herdr-shepherd"
        self.assertEqual(launcher.resolve(), checkout / "bin/herdr-shepherd")
        result = subprocess.run([str(launcher), "test", "--cwd", str(self.home), "--boss-agent", "codex",
                                 "--worker-agent", "codex", "--workers", "1", "--dry-run"],
                                env=self.env, cwd=self.home, text=True, capture_output=True, timeout=10)
        self.assertEqual(result.returncode, 0, result.stderr)
        commands = [shlex.split(line) for line in result.stdout.splitlines() if line.startswith("  herdr")]
        launches = [shlex.split(command[4]) for command in commands if command[1:3] == ["pane", "send-text"]]
        self.assertEqual(len(launches), 2)
        for argv in launches:
            self.assertIn(str(checkout / "skills/herdr-shepherd/SKILL.md"), argv[-1])
            self.assertNotIn(str(ROOT), argv[-1])


if __name__ == "__main__":
    unittest.main()
