# Changelog

What changed in MCC Rust Rewrite, newest first, written for players. Each line names its
GitHub issue where there is one. Entries are grouped by the day they reached master; a
release holds everything up to its day. Entries before 2026-11-09 were written afterwards
from the merge history.

Releases: **beta-0.0.3** holds everything up to and including 2026-11-10; beta-0.0.1 was
2026-11-06.

## 2026-11-10

### Added
- The map menu shows the game's version in the top right (#40).
- The map menu opens on the last map you started (#13).
- The Campaign tab has a difficulty choice (Y or F2), kept for next time (#17).
- Campaign runs show a clock in the top right, and the completion screen keeps your best time
  for each level and difficulty (#18).
- The bike's tuning menu opens from a controller with LB + X (#14).
- A clutch for the bike's Reflex physics: hold RB or E; on the gas with the clutch in, the engine
  revs to the limiter (#15).
- Settings has an Armour colour row for the Spartan on multiplayer maps (#16).
- Skating can show each trick's name and points as you land it, with the combo and the line's
  score, which fade out once the line ends; turn it on with Trick display in Settings (#24, #38).
- The dirt bike grips by what it rides on: it slides more on snow, ice and metal, grips better
  on rock and concrete, and sand and mud hold it back (#25).
- On foot, Q (B on a controller) melees, lunging at a foe a step or two away (#31).
- On foot, G (LT) throws a grenade and X (LB) picks frag or plasma; you start with two frags,
  pick more up by walking over them, and the HUD shows how many you carry (#31).
- Man cannons and gravity lifts launch the skateboard, and Halo 3's man cannons now throw you on foot and on the bike too (#29).
- Valhalla's man cannons throw you as far as they do in Halo 3, on foot, on the bike and on the
  board, and show their stream of blue light along the launch (#29).
- Maps play their own effects from the start, such as Valhalla's waterfall mist and the snow on Snowbound (#26).
- U plays Halo 3's campaign music on any map and U again fades it out; Shift+U skips to the next piece and Ctrl+U picks one at random (#27).
- Settings has mouse sensitivity, stick sensitivity, invert look and field of view, used when
  flying, on foot, on the bike and on the board (#20).
- Settings can switch between borderless fullscreen and a window, and turn vsync off (#21).
- Settings' Support page opens the data folder, copies a diagnostics summary for bug reports,
  resets the settings and deletes the converted Skate 3 and Reflex data (#22).
- Each run writes a log file, and a crash writes a crash report, in the data folder's logs
  folder (#23).

### Changed
- The Settings screen is split into pages: Games, Player, Controls, Display and Support (#20).
- The sound levels and the bike's tuning, setups and engine volume are saved with your other
  settings in the data folder, not beside the exe; ones saved beside the exe are still read (#40).

### Fixed
- Pause menu: the Dirt bike tab no longer says A backs the bike up; the brake held at a
  standstill does (#19).

## 2026-11-09

### Added
- Pause menu: the Dirt bike and Skate tabs list the keyboard keys next to the controller buttons (#10).
- Settings has a Dialogue language row: English, French, German, Italian or either Spanish (#12).

### Changed
- Pause menu: quitting to the map menu and restarting a level ask first; press the same button
  again to do it (#11).
- Skate 3 is optional at first-run setup: without it you still walk, shoot, fly and ride the
  dirt bike as Master Chief, and you can add it later in Settings for the skateboard (#1).
- The dirt bike and the board share one session marker: set it on either and both go back to it.
- The project keeps this changelog (#4).
- Settings' Skate 3 row opens the Skate 3 page once it is set up too, to convert it again or to
  go back to playing without it (#8).

### Fixed
- Warthog run: the glowing orange lumps at the foot of the trench cliffs are gone (#5).
- Pause menu: the dirt bike's session marker row no longer mentions the board when there is no
  skateboard (#6).
- Halo CE, Halo 2 and Reach maps still have Master Chief when Valhalla is not installed: he is
  borrowed from another Halo 3 map (#7).

## 2026-11-06

### Added
- Reflex's own bike simulation, with an Arcade / Reflex switch in Settings and the pause menu,
  and Reflex power, gearing and traction sliders.
- Shift+Tab and Page Up / Page Down change menu tabs; button hints follow whichever of
  keyboard or controller was used last.
- The Campaign tab is split into sections (Halo CE, Halo 3, Untested) and scrolls a row at a time.

### Changed
- The project is renamed MCC Rust Rewrite: `mcc-rust-rewrite.exe`, settings in
  `%LOCALAPPDATA%\MccRustRewrite` (moved from `Halo3Rewrite` on first launch).
- The project is licensed GPL-3.0-only.
- Reflex is read once at setup; the bike's model, engine sound and physics come from the
  imported copy instead of the Reflex install.
- Warthog run lighting: brighter tunnels, stronger auto exposure, reflections at full strength.

### Fixed
- `--shot` runs no longer crash with "Resource does not exist".

## 2026-11-04

### Added
- Halo CE's Warthog run (`h1:warthog_run`) with its enemies, explosions, objects and doors,
  music, Cortana's lines and ambience.

## 2026-11-03

### Added
- Swap straight between the bike and the board, keeping your speed.
- Real cube-map reflections and parallax.
- Enemies attack the rider.

### Fixed
- A plain `--shot` run no longer panics.

## 2026-11-02

### Added
- First-run setup runs on the game's own screen.
- Skating can be turned off in Settings.

## 2026-11-01

### Added
- Speedometer for the bike and the board, with an MPH / KPH setting.
- Username setting, shown in the kill feed.

### Changed
- Play and sound wait until the map has finished warming up.
- Warthog run: the tunnel girders swing down on their hinges.

## 2026-10-31

### Added
- First-run setup with folder pickers for MCC, Skate 3 and (optional) Reflex; without Reflex
  the bike is a silent stand-in. The menu greys out maps MCC hasn't installed.
- Settings screen on the main menu; restart the level from the pause menu.
- 120_halo's scripts, AI, animation and devices: the Warthog Run mission with its collapses,
  islands, frigate hangar finish, mission dialogue and music, and music volume sliders.
- Halo CE and Halo: Reach maps in the menu; Reach's large collision (Forge World and others).
- Orange CRASHED text while the bike is down.

### Fixed
- The bike engine stops when the map unloads.
- Warthog run: falling tunnel pieces land on the road, settle and stay solid.

## 2026-10-30

### Added
- A map menu (Halo 3 / Halo 2 / Campaign tabs with MCC's own map pictures); maps load behind a
  loading screen, and Q in the pause menu goes back to the menu.
- The dirt bike's engine sound (Reflex's 450F recordings, or our own synth) with a volume slider.
- Reflex's Dakumoto 450F in the Cortana kit.
- Analog throttle on the bike.
- A standing enemy Spartan to shoot, with shields, health and a kill feed; unlimited ammo.
- Halo 2 Lockout with its lightmaps, fog, props, gravity lift, weapons and ambient sound.
- A red flag where the session marker is set.

### Changed
- Back up on the bike by holding LT at a standstill.
- Every map starts with the sniper rifle.
- On-foot gravity and jump height tuned.

## 2026-10-29

### Added
- Jump off the bike or board onto foot keeping your speed, and back on from walking.
- Skate 3's session marker on the dirt bike.
- Reflex v1 bike physics, a tuning menu and five saved setup slots.
- Spartan on foot with weapons.

### Changed
- Better board roll and landing sounds.
- The game window opens borderless fullscreen.

## 2026-10-28

### Added
- First playable build: Halo 3 maps read from the player's MCC, noclip flying, Skate 3 skating
  and a dirt bike.
- Halo 3 multiplayer maps Guardian, Last Resort, High Ground and Blackout.
- Halo 2 maps (Coagulation first), with skating.
- A pause menu with controls and free roam, dirt bike and skate tabs.
