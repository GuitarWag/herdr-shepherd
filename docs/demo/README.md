# Password reset workflow GIF

An illustrated version of the main README example: one Codex boss delegates two investigations to Claude Code and Codex workers, reviews their findings, and updates clitasks. Dialogue and timing are examples; this is not a terminal recording.

The launch command and task names come from [the project README](../../README.md). The task rows illustrate the boss running clitasks commands as instructed; shepherd does not synchronize task state automatically.

## Edit and preview

Requires Node.js 22+, FFmpeg, and a Chromium browser supported by HyperFrames.

```sh
cd docs/demo
npm run dev
```

`index.html` mounts the editable scene in `compositions/components/code-terminal-run.html`. `SPEC.md` records the visual and timing decisions. The text reveal is adapted from the HyperFrames registry's `code-terminal-run` component.

## Render

From this directory:

```sh
sh render.sh
```

This checks the composition, renders a 1920×1080 master into the ignored `renders/` folder, and writes the 960×540 looping GIF to `../assets/password-reset.gif`.
