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
- The S3 shots use moto's S3 server (`moto_server -p 5055`, keys
  `testing`/`testing`) seeded with a few buckets; the profiles were
  imported through the tool's "Import AWS Profiles" from a `~/.aws` pair
  pointing at it. The tunnel shot forwards a local port through the same
  OpenSSH server to a small HTTP service.
- `hyperstack-demo.gif` is an `ffmpeg x11grab` recording of one scripted run
  (vault unlock → launcher → terminal with search and a Ctrl+clicked
  `path:line` → editor at that line → Markdown preview with a Mermaid
  diagram → SSH/SFTP and a new tunnel → S3 bucket browsing and a presigned
  URL → Production environment),
  converted at 10 fps, 1200 px wide with a generated palette.
- PNGs are quantised to 256 colours to keep the repository small.

Retake them when the UI changes visibly; keep the file names, the README
links to them.
