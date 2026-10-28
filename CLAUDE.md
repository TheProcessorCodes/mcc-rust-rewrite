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
