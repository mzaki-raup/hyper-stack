# Screenshots

Images used by the top-level [README](../../README.md). They show HyperStack
0.11.0 as built from this repository, not mock-ups.

## How they were made

- Built with `cargo build --release -p hyperstack` on Linux x64 and run under
  Xvfb at 1440×900 (Mesa software rendering), driven with `xdotool`.
- Fresh portable data folder (`HyperStackData/` beside the executable); the
  vault, the SSH connection profile and the folder sync were created and
  exercised through the UI. The three environments (Development, Staging,
  Production) and a second, illustrative SSH profile (`db-01 (production)`)
  were written as workspace files first.
- The SSH/SFTP shots use a local OpenSSH server (`127.0.0.1:2200`, user
  `deploy`) so the shell and SFTP pane are real; the shell prompt sets the
  xterm title, which is what **Follow Shell Folder** reads.
- `hyperstack-demo.gif` is an `ffmpeg x11grab` recording of one scripted run
  (vault unlock → launcher → terminal → editor + Markdown preview → command
  palette → SSH/SFTP → Production environment → Settings → Environments),
  converted at 10 fps, 1200 px wide with a generated palette.
- PNGs are quantised to 256 colours to keep the repository small.

Retake them when the UI changes visibly; keep the file names, the README
links to them.
