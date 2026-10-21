"""Install local links without replacing existing files or links."""

import argparse
from pathlib import Path
import re
import shutil
import subprocess
import sys


ROOT = Path(__file__).resolve().parent.parent
MIN_HERDR = (0, 7, 3)


def links():
    home = Path.home()
    binary = home / ".local/bin"
    skill = ROOT / "skills/herdr-shepherd"
    return {
        binary / "herdr-shepherd": ROOT / "bin/herdr-shepherd",
        binary / "herd-boss": ROOT / "bin/herdr-shepherd",
        binary / "herd-view": ROOT / "bin/herd-view",
        home / ".claude/skills/herdr-shepherd": skill,
        home / ".agents/skills/herdr-shepherd": skill,
    }


def dependencies():
    errors = []
    if sys.version_info < (3, 9):
        errors.append("Python 3.9 or later is required.")
    for command in ("bash", "herdr"):
        if not shutil.which(command):
            errors.append(f"{command} is not on PATH.")
    if not any(shutil.which(command) for command in ("claude", "codex")):
        errors.append("Install Claude Code or Codex and add its command to PATH.")
    if shutil.which("herdr"):
        response = subprocess.run(["herdr", "--version"], capture_output=True, text=True, timeout=10, check=True)
        match = re.search(r"\b(\d+)\.(\d+)\.(\d+)\b", response.stdout)
        if not match or tuple(map(int, match.groups())) < MIN_HERDR:
            errors.append("herdr 0.7.3 or later is required.")
    return errors


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("mode", nargs="?", choices=("install", "check"), default="install")
    args = parser.parse_args()
    errors = dependencies()
    targets = links()
    for target, source in targets.items():
        if target.is_symlink() and target.resolve() == source.resolve():
            continue
        if target.exists() or target.is_symlink():
            errors.append(f"Preserve the existing path. Move it before installation: {target}")
        elif args.mode == "check":
            errors.append(f"The link is missing: {target}")
    if errors:
        for error in errors:
            print(f"herdr-shepherd: {error}", file=sys.stderr)
        return 1
    if args.mode == "install":
        for target, source in targets.items():
            if target.is_symlink():
                continue
            target.parent.mkdir(parents=True, exist_ok=True)
            target.symlink_to(source)
            print(f"Linked {target}")
    print("The commands and both skill links are ready.")
    print('If needed, add ~/.local/bin to PATH: export PATH="$HOME/.local/bin:$PATH"')
    print("Start herdr from a terminal before you create a team.")
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (OSError, ValueError, subprocess.SubprocessError) as error:
        print(f"herdr-shepherd: {error}", file=sys.stderr)
        sys.exit(1)
