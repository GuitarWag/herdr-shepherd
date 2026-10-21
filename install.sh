#!/bin/sh
# Install the commands and the shared agent skill.
set -eu
here=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
exec python3 "$here/scripts/install.py" "$@"
