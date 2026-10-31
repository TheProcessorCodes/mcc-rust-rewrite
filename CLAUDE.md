Read `HANDOFF.md` before doing anything else. It holds the goals, the research done so
far, the paths on disk and the open decision.

# Standing rules

- **Do not launch or run the game to test** unless the user says so. Building is fine.
  The user playtests themselves; hand them a PowerShell line with the path quoted and `&`
  in front.
- **Showing the user screenshots.** The user often reads the thread on their phone, where a
  Markdown image of a local path (`![](D:\...\shot.png)`) does not show. Put the pictures in
  a page instead: load `mcp__t3-code__html_preview` and `mcp__t3-code__html_render` with
  ToolSearch, write a small HTML page with one `<img src="D:\full\path\shot.png">` per
  picture (absolute paths; the tool inlines local images) and a short caption under each,
  check it with `html_preview`, then show it with `html_render`, passing the
  `contentHeight` that `html_preview` reported as `height`. Use theme CSS variables
  (`var(--radius)`, `var(--muted-foreground)`), no page background or outer card; three or
  four full-size PNGs per page is fine. Take the pictures headless (`halo3.exe <map> --shot
  <dir>`, `halo3.exe --menu-shot FILE ...`, see README), never by opening a window. For
  mock-ups before any code exists, draw them as HTML/CSS in the same kind of page.
- **Game installs are read-only.** Never modify, patch or write into the MCC folder, the
  MW2 folder or the Skate 3 dump. Caches and outputs go inside this project.
- **No game files in this repo.** The user brings their own copies; the code reads them at
  runtime, the way IW4L does.
- **Build for a public release.** Assume a stranger downloads this and has only their own
  MCC install and their own Skate 3 copy. Nothing may depend on this PC's paths, the
  `mw2 skate 3` folder or game data in the repo or the release. A one-time first-run setup
  finds or asks for MCC and Skate 3, checks them, converts what it needs into a cache next to
  the exe, saves the paths, and never asks again (the IW4L `launcher/src/first_run.rs` model).
  New code reads game paths only from that saved setup, never a hard-coded `C:\`/`D:\` path;
  env vars stay as dev overrides. Everything Halo needs is read from MCC at run time;
  everything Skate 3 needs comes out of the user's Skate 3 copy. Don't commit files
  extracted from either game (textures, models, audio, dumps of tag data).
- **Code elsewhere on disk is reference, not ours to edit.** `mw2-skate-3-src`,
  `D:\reflex-tools` and the Reclaimer clone belong to other work. Read them and copy what
  helps into this project; do not change them without asking.
- **Licenses**: Gravemind2401's Reclaimer is GPL-3.0. Porting its code line by line makes
  that code GPL-3.0. IW4L is Apache-2.0. Raise this with the user before porting Reclaimer.
  The Skate 3 engine is vendored in `third_party/skate-engine` (GPL-3.0-only, so a released
  `halo3.exe` is GPL-3.0); the skate crates are built from there, not from `mw2-skate-3-src`.
  Any change made there goes in the list in its README.
- **New work goes in a new worktree.** At the start of a thread, before editing
  anything, create a worktree on a new branch from master (under `.claude/worktrees/`)
  and do the work there, not on master. Long threads may need more than one: once a
  thread's worktree has been merged, start a fresh worktree for the next piece of work
  instead of reusing the merged one. If you are unsure whether something needs its own
  worktree, ask the user.
- **Remove a worktree once it is merged to master.** After merging, check that it has no
  uncommitted changes (`git -C <path> status --porcelain`), then `git worktree remove
  <path>` and `git branch -d <branch>`. Never remove a worktree whose branch has commits
  master doesn't (`git merge-base --is-ancestor <branch> master` fails), or one with
  uncommitted work, without asking the user.
- **At most two builds at once.** Run every `cargo build`/`run`/`test`/`check` through
  the gate, from any worktree, using the main checkout's copy:
  `& 'D:\GAME ISO AND ROMS\PC - AI MASHUP\halo-3-rust-rewrite\.dev\build-gate.ps1' cargo build --release`
  (from Bash: `powershell -NoProfile -File '<same path>' cargo ...`). If two builds are
  running it prints its place in the queue and waits, then runs. Give the tool call a long
  timeout or run it in the background. Notices also go to `.dev\build-gate.log`.
  `.dev\` is dev-only and not in git (listed in `.git/info/exclude`); if the script is
  missing, build normally.
- **Every checkout builds into the main checkout's `target\`**, so bevy is compiled once.
  From inside the checkout (main or worktree) run
  `& 'D:\GAME ISO AND ROMS\PC - AI MASHUP\halo-3-rust-rewrite\.dev\build-shared.ps1'`
  (default `cargo build -p halo3`; pass other cargo arguments, e.g. `test -p halo3_bike`).
  It goes through the gate, rebuilds our crates from this checkout when the last build came
  from another one, and copies the exe to that checkout's `out\bin\halo3.exe`. Hand the user
  that `out\bin` path to play, never `target\debug\halo3.exe`, which belongs to whichever
  checkout built last. Don't point plain cargo at the shared `target\`: the shared target
  keeps one copy of each of our crates, and without the script cargo can reuse another
  checkout's copy without noticing.
