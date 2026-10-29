Read `HANDOFF.md` before doing anything else. It holds the goals, the research done so
far, the paths on disk and the open decision.

# Standing rules

- **Do not launch or run the game to test** unless the user says so. Building is fine.
  The user playtests themselves; hand them a PowerShell line with the path quoted and `&`
  in front.
- **Game installs are read-only.** Never modify, patch or write into the MCC folder, the
  MW2 folder or the Skate 3 dump. Caches and outputs go inside this project.
- **No game files in this repo.** The user brings their own copies; the code reads them at
  runtime, the way IW4L does.
- **Code elsewhere on disk is reference, not ours to edit.** `mw2-skate-3-src`,
  `D:\reflex-tools` and the Reclaimer clone belong to other work. Read them and copy what
  helps into this project; do not change them without asking.
- **Licenses**: Gravemind2401's Reclaimer is GPL-3.0. Porting its code line by line makes
  that code GPL-3.0. IW4L is Apache-2.0. Raise this with the user before porting Reclaimer.
- **New work goes in a new worktree.** At the start of a thread, before editing
  anything, create a worktree on a new branch from master (under `.claude/worktrees/`)
  and do the work there, not on master. Long threads may need more than one: once a
  thread's worktree has been merged, start a fresh worktree for the next piece of work
  instead of reusing the merged one. If you are unsure whether something needs its own
  worktree, ask the user.
- **At most two builds at once.** Run every `cargo build`/`run`/`test`/`check` through
  the gate, from any worktree, using the main checkout's copy:
  `& 'D:\GAME ISO AND ROMS\PC - AI MASHUP\halo-3-rust-rewrite\.dev\build-gate.ps1' cargo build --release`
  (from Bash: `powershell -NoProfile -File '<same path>' cargo ...`). If two builds are
  running it prints its place in the queue and waits, then runs. Give the tool call a long
  timeout or run it in the background. Notices also go to `.dev\build-gate.log`.
  `.dev\` is dev-only and not in git (listed in `.git/info/exclude`); if the script is
  missing, build normally.
