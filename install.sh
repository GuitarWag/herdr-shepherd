#!/bin/sh
# Install the commands and the shared agent skill.
set -eu
here=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
if [ ! -x "$here/bin/herdr-shepherd" ]; then
    echo 'Build the binary first: cargo build --release --locked' >&2
    exit 1
fi
exec "$here/bin/herdr-shepherd" --install "$@"
