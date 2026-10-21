"""Copy the compiled package without its source or build dependencies."""

import shutil


def copy_package(source, target):
    (target / "bin").mkdir(parents=True)
    shutil.copy2(source / "bin/herdr-shepherd", target / "bin/herdr-shepherd")
    for name in ("herd-boss", "herd-view"):
        (target / "bin" / name).symlink_to("herdr-shepherd")
    shutil.copytree(source / "skills", target / "skills")
