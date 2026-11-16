#!/bin/sh
set -eu
cd -- "$(dirname -- "$0")"
npx --yes hyperframes@0.9.56 check
mkdir -p renders ../assets
npx --yes hyperframes@0.9.56 render --quality delivery --fps 30 --output renders/password-reset.mp4
ffmpeg -y -i renders/password-reset.mp4 -filter_complex "fps=12,scale=960:-1:flags=lanczos,split[a][b];[a]palettegen=max_colors=256:stats_mode=full[p];[b][p]paletteuse=dither=none:diff_mode=rectangle" -loop 0 ../assets/password-reset.gif
