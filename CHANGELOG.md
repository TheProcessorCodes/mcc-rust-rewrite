# Changelog

What changed in MCC Rust Rewrite, newest first, written for players. Each line names its
GitHub issue where there is one. There are no numbered releases yet, so entries are grouped
by the day they reached master. Entries before 2026-11-09 were written afterwards from the
merge history.

## 2026-11-09

### Changed
- Skate 3 is optional at first-run setup: without it you still walk, shoot, fly and ride the
  dirt bike as Master Chief, and you can add it later in Settings for the skateboard (#1).
- The dirt bike and the board share one session marker: set it on either and both go back to it.
- The project keeps this changelog (#4).

### Fixed
- Warthog run: the glowing orange lumps at the foot of the trench cliffs are gone (#5).

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
