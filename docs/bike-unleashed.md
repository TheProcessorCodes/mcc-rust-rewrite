# The dirt bike: MX vs ATV Unleashed's model

Written 2026-10-28. The viewer's dirt bike (`crates/mcc_bike/src/physics.rs`, numbers in
`unleashed.rs`) rides the way MX vs ATV Unleashed's stock Pro 250 does. This page says how the
original works in plain words, where each fact came from, and what our version leaves out.

The bike physics from before this rewrite is kept on the branches
`backup/bike-physics-master-2026-10-28` (master's Reflex-style bike with the air wheel) and
`backup/bike-physics-reflex-2026-10-28` (the unmerged stock-Reflex model, engine and tuning menu).

## Sources

- **The tuning data**: the user's own Unleashed install (Steam,
  the Unleashed install folder). `data\common.pak` holds
  `Data\Veh\mx_*.phy`, plain INI text, one per bike (Pro / Race, 50 / 125 / 250 / 500, `A` and `P`
  variants that differ in a few tyre values). The `.pak` format: a `u32` entry count, then per
  entry a 100-byte name, `u32` offset and `u32` size; offsets count from the end of that table.
  Units are feet, pounds, mph and degrees.
- **How the game uses it**: the fan client "MX vs ATV custom client 2.0.2" ships `mxacore.dll`,
  a C reimplementation of Unleashed's physics with its function names kept (suffixes look like the
  original executable's addresses). We decompiled it with Ghidra headless into the git-ignored
  `cache\mxacore\` (scripts there) and read the bike functions. No code was copied: the model below
  is written in our own words and implemented from scratch. `mxacore.dll` is someone else's
  unreleased work built from a commercial game; treat it as a reference only.

## The model

Everything acts on one rigid body (bike and rider, 420 lb = 190.5 kg, inertia 22.4 / 18.4 /
10.9 slug·ft² about pitch / yaw / roll). Gravity is 1.03 g. Yaw rate is capped at 3 rad/s.

**The rider's weight** (`moto_set_cofg`). The centre of gravity sits 2 ft above the ground point
under the bike. Pulling the stick back moves it up to 1.1 ft back; pushing forward moves it
0.05 ft forward.

**Bars** (`moto_pre_process`). On the ground the bar angle is set straight from the stick:
30° × a speed scale running from 0.9 at a standstill to 0.3 at 75 mph. The stick itself moves at
most 4 units a second.

**Lean** (`moto_post_process`). The physics never lets the bike roll on the ground: the rigid
body's roll axis is locked whenever a wheel touches. After each step the orientation is rebuilt:
the lean aims for stick × (0° at rest up to 45° at 35 mph), approached at 5 per second. It is
measured from a reference "up" that is the vertical below 10 mph, the ground's normal above
30 mph, and a blend between.

**Suspension** (`tire_probe_ground`, `tire_contact_force`). Each wheel is a circle in the bike's
plane. Its lowest point towards the ground (the ground's normal with the sideways part removed)
is compared with the ground plane; the overlap along that direction is the compression. The
spring pushes along the ground's normal at the contact: force = k · compression + c · rate, with
k = `Spring` × the bike's weight per foot (1.65 front, 2.55 rear) and c a share of critical
damping (0.3 front, 0.4 rear). The rate is clamped to 20 ft/s. Past 1 ft of compression the frame
bottoms out and is pushed back up. The wheel drops back out no faster than 4 ft/s, which only
matters for drawing.

**Tyre grip** (`tire_traction`, `tire_slip_forces`, `tire_wheel_torque`). The grip forces act
not at the contact patch but at the centre of gravity, plus a twist as if applied at
`BodyTireForcePosition` (0.25 ft down, 0.75 ft fore or aft). So the tyres steer the bike but
hardly tip or pitch it.
- Along the tyre: the wheel's torque (engine, minus rolling friction and the brake) over the
  radius, up to 2 × the load. Asked for more, the tyre breaks loose and gives 1.9 × the load:
  wheelspin or a locked wheel. The brake's torque (200 lbf·ft per wheel) fades in with speed,
  `clamp(0.3 · v ft/s, -1, 1)`, so it holds without reversing.
- Sideways: grip against slip speed in ft/s, `-SS · Scale · v / (1 + |SS · RoundOff · v| ^ DropOff)`.
  Stock dirt: Scale 38.2, SpeedScale 0.005, RoundOff 4, DropOff 1.5. That peaks near 5 × the load
  at about 100 ft/s of slip and falls away after. Below 10 ft/s of rolling speed the slip counts
  up to 5 times (stiffer when nearly stopped). There are three curves: rolling, slipping (spin or
  lock) and braking. The game has 16 surfaces ("friction zones"); we use zone 1, on-track dirt.
- The rear wheel turns with the ground unless the tyre is loose, when the leftover torque spins
  it up or down.

**Engine** (`engine_update`, `moto_auto_shift`). Torque from the HP table (HP × 5252 / rpm, peak
about 34 lbf·ft at 7,500 rpm, limiter 9,500, never below 4,500). Five gears (1.8 to 0.869) × a
10.538 final drive. A clutch slips at 60 lbf·ft until the wheel's rpm crosses the engine's, then
locks; it unlocks if the engine asks more than 70. Engine braking grows with rpm². Shifts are
automatic: up past 8,500, down below 6,500 rpm (from road speed). Below 5 mph with no gas the
clutch is in. Flat out it reaches about 80 mph against drag of ½ · 0.025 · v² lbf (v in ft/s).

**Wheelie hold** (`moto_pitch_control`). With the rear down, gas on, over 5 ft/s, no brake and
the stick at least half back, a PI controller holds the pitch at (2 × stick − 1) × 40°, fading
out towards 120 mph. **Stoppie hold**: with the front down, brake on, over 15 ft/s and the stick
fully forward, the same holds up to 88° nose down; the bars still yaw the bike about the front
tyre.

**In the air** (`moto_airborne_control`). Stick torques: pitch 400 lbf·ft from the stick's Y,
yaw 500 and roll 150 from X, against 450 lbf·ft per rad/s of damping. With X let go it levels the
lean and turns to face the way it is travelling instead. The rear brake drops the nose (400 lbf·ft),
the gas lifts it (175). X also gives a 125 lbf sideways push.

**Backing up** (`moto_reverse_force`): a push of 30 lbf per ft/s towards -10 mph.

## Reflex v1 and the tuning menu (2026-10-29)

The bike now rides on **Reflex v1** by default (`crates/mcc_bike/src/reflex.rs`): the model
above, unchanged in how it simulates, with stock MX vs ATV Reflex's mx450f numbers and four of
Reflex's rules (as the earlier Reflex-style bike used):

- Reflex's mass (177 kg), inertia, springs (forks 20.2 kN/m, shock 13.9 kN/m), bump / rebound
  dampers, travel (0.40 / 0.37 m), brakes (339 / 475 N·m), torque curve (55 N·m, 11,500 rpm
  limiter, 1,500 idle), five overall ratios (scaled for our tyre) and ground drag. Gravity 1 g.
- The shape of the bike drawn, Reflex's Dakumoto 450F (2026-10-29): its axles (1.46 m wheelbase),
  tyres (0.345 / 0.335 m), rake (29.2°), grips, pegs and probe spheres. `docs/format/reflex-bike.md`
  has the measurements and the before / after table.
- The bars follow Reflex's steering-for-speed curve (30° standing, 9.5° at 50 mph), the stick
  squared and lagged 0.15 s; the rider leaning sideways adds 20 % to the bars.
- The lean follows Reflex's lean-for-speed curve (5.6° standing to 34° at 80 mph), lagged
  0.3 s; the rider leaning into the turn adds 55 %.
- Sideways grip is capped at 1.3 × load (Unleashed climbs to about 5) and the curve is 2.5 ×
  stiffer, so it reaches the cap at small slip; past the cap the tyre slides at 0.9 of it.
  Drive grip is 1.3 × load (1.2 spinning).

Result on flat ground: about 25 mph in 2 s (wheelspin holds the launch to about 0.6 g), top
speed near 32 m/s (72 mph), as Reflex's model gives.

**0** while riding opens the tuning menu (`tune.rs`): the first slider switches the base between
Unleashed's Pro 250 as it was and Reflex v1; the rest scale engine power, gearing, engine braking,
brakes, grip, side stiffness, slide grip, steering lock and lag, lean depth and speed, rider-lean
influence, weight shift, wheelie hold, spring, damping, air rotation, air damping and gravity.
Each row reads out what the value means on the tuned bike. Closing the menu saves the values to
`bike-tuning.json` next to the exe; the next start loads them (Delete puts all back to stock).
Under the sliders are five setup slots, kept in `bike-setups.json` next to the exe: Shift+1 to 5
(or Ctrl) saves the current setup to a slot and 1 to 5 loads one, or pick the "Save to slot" /
"Load slot" row, choose a slot with A / D and press Enter (A on a pad), or click a slot.

## Not done yet

- Surfaces: done 2026-11-10 (#25) with Reflex's grip and drag classes rather than Unleashed's
  zones: the ground's kind (`surface.rs`, from the Halo material) scales the tyres' grip and
  adds terrain drag, on this model and the Reflex one (HANDOFF.md, "Reflex's own simulation").
- Wheelies come slowly: from a standing start, full gas with the stick back lifts the front
  after about 1.5 s, at about 33 mph (the hold fades with speed). The game moves the rider's weight
  another 1.4 ft back for a moment when the clutch is dumped (`ClutchCofGTravelBack`), which is
  probably what makes its wheelies pop; that is not ported yet.
- Clutch boost (holeshot: up to 3.3 × torque for 1.5 s after dumping the clutch), preload jumps,
  backflips, 360s, donuts, no-hander balance, the low-speed side-slip killer, and Unleashed's
  wreck rules (angle and speed limits); we keep our own probe crashes.
- `SpringWhenStopped` (a softer spring at rest) is read by the game but we did not find where it
  is used.
- The axle positions come from the bike model's wheel nodes (`Bike.vue`), not from the physics.
- Other bikes (125, 500, the Race tunes) would only need their `.phy` values; loading them from
  the user's install at runtime would avoid copying numbers into the code.
