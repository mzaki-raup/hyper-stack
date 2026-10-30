---
description: Record a mistake in docs/GOTCHAS.md under the right heading, in the file's own style
argument-hint: "<what went wrong and what fixed it>"
---

# /gotcha

Mistake: $ARGUMENTS

1. Grep `docs/GOTCHAS.md` for the symptom first. If an entry already covers it,
   sharpen that entry instead of adding a second one, and say which you chose.
2. Pick the matching heading — the existing ones are: Git, Merges and Releases;
   Release Pipeline and Updater; Patch Scripts and Text Editing; Windows
   Environment and Build; Build and Dependency Gotchas (Rust); Clippy and
   Lints; Shell and UI Layout (egui); the per-tool sections (Terminal and
   Editor, Databases, Logs, Containers, Graphics and Diagrams, AI Assistant,
   API Client, Browser, SFTP and Connection Forms, S3); Headless and GUI
   Testing; Security and Vault; Cross-Platform; Logs and subprocesses. Only add
   a new heading if none of those fits, and say why.
3. Write it in the file's own shape: a bold lead naming the trap, then the
   symptom as it actually appeared, then the fix — one or two sentences,
   concrete, with the real error text or the real numbers. Someone hitting this
   again should recognise it from the symptom alone.
4. Edit the file with the Edit tool. Append the entry at the end of its
   section, matching the surrounding bullet style.
5. If the mistake also changed behaviour, add the `CHANGELOG.md` line, and
   reference the ledger item it came out of (`BUG-0003`) in the entry if there
   is one. If the trap deserves a fix that hasn't been scheduled yet, file it
   with `/task-new bug` rather than leaving it only as prose.
