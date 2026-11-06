This is **MCC Rust Rewrite** (`mcc-rust-rewrite.exe`, crates `mcc_*`; called "Halo 3 Rust
rewrite", `halo3.exe` and `halo3_*` until 2026-11-06): Halo maps from the player's own MCC with
Skate 3 skating and the MX vs ATV Reflex dirt bike.

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
  four full-size PNGs per page is fine. Take the pictures headless (`mcc-rust-rewrite.exe <map> --shot
  <dir>`, `mcc-rust-rewrite.exe --menu-shot FILE ...`, see README), never by opening a window. For
  mock-ups before any code exists, draw them as HTML/CSS in the same kind of page.
- **Game installs are read-only.** Never modify, patch or write into the MCC folder, the
  MW2 folder or the Skate 3 dump. Caches and outputs go inside this project.
- **No game files in this repo.** The user brings their own copies; the code reads them at
  runtime, the way IW4L does.
- **Build for a public release.** Assume a stranger downloads this and has only their own
  MCC install and Skate 3 copy, and maybe MX vs ATV Reflex. Nothing may depend on this PC's paths, the
  `mw2 skate 3` folder or game data in the repo or the release. The one-time first-run setup
  (`crates/mcc_setup`, README "First run") asks the player to choose the folder of MCC, Skate 3 (extracted or `.iso`) and Reflex in
  a folder picker (never searches the disk for them), MCC and Skate 3 required to play, Reflex optional (without it the bike is our own model and sound), checks them, converts what it needs, and saves it all per user in
  `%LOCALAPPDATA%\MccRustRewrite`, so it never asks again. New code gets game paths only
  through `crates/game_paths` (`halo3_maps()`, `halo3_map(name)` for tests, `skate_assets()`,
  ...), never a hard-coded `C:\`/`D:\` path; env vars stay as dev overrides. Tests skip quietly
  when a game is not installed, and code still must not crash if a file is missing. Everything Halo needs is read from MCC at run time;
  everything Skate 3 needs comes out of the user's Skate 3 copy, everything Reflex needs out of
  their Reflex. Don't commit files extracted from any of them (textures, models, audio, dumps of tag data).
- **Code elsewhere on disk is reference, not ours to edit.** `mw2-skate-3-src`,
  `reflex-tools` and the Reclaimer clone belong to other work. Read them and copy what
  helps into this project; do not change them without asking.
- **Licenses**: this project is GPL-3.0-only (workspace `license` in `Cargo.toml`, text in
  `LICENSE`). Gravemind2401's Reclaimer is GPL-3.0; the user chose not to port its code, so
  raise it with them before porting Reclaimer. IW4L is Apache-2.0, which may be used in a
  GPL-3.0 work: keep its notice in files copied from it, and keep `LICENSE-APACHE`.
  The Skate 3 engine is vendored in `third_party/skate-engine` (GPL-3.0-only, so a released
  `mcc-rust-rewrite.exe` is GPL-3.0); the skate crates are built from there, not from `mw2-skate-3-src`.
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
  Then delete the old crate versions the builds left in the shared `target\debug\deps`
  (cargo never removes them; they had reached 234 GB):
  `& "$(git rev-parse --path-format=absolute --git-common-dir)/../.dev/prune-deps.ps1"`.
  It asks cargo which artifacts the main checkout's build and tests use, keeps those and
  anything from the last 24 hours, and deletes the rest. It also deletes the compile
  caches in `target\debug\incremental` that no build has used for 48 hours (they had
  reached 72 GB). `-DryRun` lists only. It may rebuild our crates first. If the script is
  missing, skip this step.
- **At most two builds at once.** Run every `cargo build`/`run`/`test`/`check` through
  the gate, from any worktree, using the main checkout's copy:
  `& "$(git rev-parse --path-format=absolute --git-common-dir)/../.dev/build-gate.ps1" cargo build --release`
  (the git command finds the main checkout from anywhere; from Bash:
  `powershell -NoProfile -File "$(git rev-parse --path-format=absolute --git-common-dir)/../.dev/build-gate.ps1" cargo ...`). If two builds are
  running it prints its place in the queue and waits, then runs. Give the tool call a long
  timeout or run it in the background. Notices also go to `.dev\build-gate.log`.
  `.dev\` is dev-only and not in git (listed in `.git/info/exclude`); if the script is
  missing, build normally.
- **Every checkout builds into the main checkout's `target\`**, so bevy is compiled once.
  From inside the checkout (main or worktree) run
  `& "$(git rev-parse --path-format=absolute --git-common-dir)/../.dev/build-shared.ps1"`
  (default `cargo build -p mcc-rust-rewrite`; pass other cargo arguments, e.g. `test -p mcc_bike`).
  It goes through the gate, rebuilds our crates from this checkout when the last build came
  from another one, and copies the exe to that checkout's `out\bin\mcc-rust-rewrite.exe`. Hand the user
  that `out\bin` path to play, never `target\debug\mcc-rust-rewrite.exe`, which belongs to whichever
  checkout built last. Don't point plain cargo at the shared `target\`: the shared target
  keeps one copy of each of our crates, and without the script cargo can reuse another
  checkout's copy without noticing.
