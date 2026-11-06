# MX vs ATV Reflex: engine sounds

What `reflex_assets::engine_audio` reads and `mcc_audio::engine` plays. Worked out
2026-10-30 from the user's Steam install (read-only); nothing from it is copied into
this project.

## Where

`<Reflex>\AudioPackagesList.xml` lists the audio databases. Engines are in
`Database\VehicleEngineAudio.dx9.database` / `.dx9.package` (the pair format is in
`reflex-bike.md`): one package per engine size (`MXVehicleEngineSize_MX125cc`,
`_MX250cc`, `_MX250f`, `_MX450f`, `_ATV450f`, `_UTV660cc`, `_Pro2V8`, `_ProLiteLS7`,
`_SuperBuggy16V`; `_MX50cc` and `_Undefined` are empty), each LZX-packed like the rest:

| asset | type | what |
|---|---|---|
| `MX_450F_01` | `sound` | FMOD Designer event file: a 0x40-byte Reflex header naming it `MX_450F_01.fev`, then `FEV1` |
| `MX_450F_bank01` | `soundbnk` | a 0x44-byte Reflex header naming it, then an FMOD `FSB4` bank |

The `ATV450f` package holds the same two assets again. `EngineState/AudioEngineStates`
(a bxml) maps each engine size to its event file and gives the game's engine states one
event each: `Idle`, `Rev`, `StartLine` (`REV_SCN`), `AccelLight`, `AccelPwr`,
`AccelHalf`, `SustainHalf`, `SustainLow`, `SustainMed`, `SustainHigh`, `SustainPower`,
`DecelHigh`, `DecelLow`, `Shift`, `Downshift`, `Choke`, `Flooding`, `PanicStart`,
`PanicLoop`, `EnterAir`, `Landing`, `Wreck` (`Drive`, `ReverseStart`, `ReverseLoop`
unused on bikes). Foley (tyres, suspension, rider) is in `VehicleFoleyAudio`
(`VEHICLE_FOLEY_bank01`, `RIDER_AUDIO_bank00`), not read yet.

## FSB4

From the `FSB4` magic:

| offset | size | |
|---|---|---|
| 0 | 4 | `FSB4` |
| 4 | 4 | sample count (54 for the 450F) |
| 8 | 4 | bytes of sample headers (54 × 80) |
| 12 | 4 | bytes of sample data |
| 16 | 4 | version 0x00040000 |
| 20 | 4 | bank mode (0x20) |
| 24 | 24 | hash, GUID |

Then one header per sample (the first u16 is its size, 80 here): +2 the file name, 30
bytes (`MXB_450F_IDLE.wav`); +32 length in samples; +36 bytes of data; +40 / +44 loop
start and end (the whole take on all 54); +48 mode; +52 rate. The data follows the
headers, each sample straight after the last.

All 54 of the 450F are mode 0x100220 (MPEG, mono), 44,100 Hz, MPEG-1 layer III at
96 kbit/s (313 or 314-byte frames). FSB pads between the frames with zero bytes; a
decoder needs them taken out (step from one frame header to the next by the frame
length, skipping to the next `FF FB` sync). Then the decoded length matches the header
(the IDLE's 131,328 samples, plus the decoder's own delay).

## The 450F's takes

Measured pitch (the firing frequency, the spacing of its harmonics on 0.2 s windows;
rpm = frequency × 120 for a four-stroke single):

| takes | rpm | notes |
|---|---|---|
| `IDLE` | 3,690 | 3 s; quieter than everything else |
| `HALF_ACCEL`, `HALF_SUSTAINED` | 3,420, 4,260 | light throttle, low revs; `HALF_SUSTAINED` is dead steady |
| `THROTTLE_LOW_01`..`06` | 7,050 to 10,035 | |
| `THROTTLE_MID_01`..`07` | 8,085 to 10,140 | |
| `THROTTLE_HIGH_01`..`10` | 8,640 to 11,010 | |
| `THROTTLE_POWER_01`..`09` | 5,745 to 8,850 | wander most (spread 10 to 18 %); `01`, `06`, `07` are used |
| `DECEL_HIGH`, `DECEL_LOW` | about 9,900 falling to 4,300 in 1.1 s | |
| `PANIC_START`, `PANIC_LOOP`, `ENTER_AIR` | 11,250 to 11,520 | on the limiter; Reflex v1's limiter is 11,500 |
| `REV_01`..`04` | sweeps 7,500 to 10,000 and back to 4,300 | |
| `THROTTLE_LAND_01`..`06` (no `04`) | 6,360 to 7,950 | |
| `FLOODING_01`..`03`, `CHOKE`, `WRECK` | | `CHOKE` and `WRECK` are the same bytes |

## The event file

`FEV1` (version 0x3A): the bank name, the event categories (`master`, `music`), then one
record per event (`MXB_450F_IDLE`, `..._THROTTLE_LOW`, ...: name, GUID, volume 0.797 on
all, priority, max playbacks, 3D settings, one layer playing one sound definition, three
envelopes (an unnamed one, `FMOD Lowpass Simple`, and a third) with the same points on
every event, and on the four throttle events a parameter `load`, 0 to 1). Then three
sound-definition property templates (play mode first; the throttle pools' is 2, random
without repeats), and the sound definitions, which `reflex_assets::engine_audio::
sound_defs` reads: a u32 count, then per definition its name (`/MXB_450F_THROTTLE_LOW`,
u32-length-prefixed with a NUL), the template index, the entry count, and per entry a u32
kind and a u32 weight (100); a kind-0 entry goes on with the `.wav` name, the bank name,
its index in the bank and its length in ms (kind 2 plays nothing: `SHIFT`, `DOWNSHIFT`,
`ACCEL_LIT`, `ACCEL_PWR` are empty). The pools do not follow the take names:

| definition | takes |
|---|---|
| `THROTTLE_LOW` | `HIGH_01`, `HIGH_09`, `HIGH_08`, `LOW_03`, `MID_02`, `LOW_01`, `LOW_06` |
| `THROTTLE_MID` | `LOW_04`, `MID_03`, `MID_01`, `MID_05`, `MID_07`, `HIGH_08`, `POWER_01` |
| `THROTTLE_HIGH` | `HIGH_02` to `_08`, `HIGH_10`, `POWER_07`, `MID_04`, `MID_06` |
| `THROTTLE_POWER` | `LOW_02`, `LOW_05`, `POWER_02` to `_06`, `POWER_08`, `POWER_09` |
| `HALF_ACCEL`, `HALF_THROTTLE` | `HALF_ACCEL`; `HALF_SUSTAINED` |
| `THROTTLE_LAND`, `REV`, `FLOODING` | their numbered takes |

`EngineState/AudioEngineStates` holds no numbers beyond a level of 1.0 per state; when
each event plays, and at what pitch, is in the game's code.

## How Reflex mixes them

From a 13 s recording of the game (the user's, 2026-10-30; riding, two jumps, two
wrecks), pitch and loudness every 0.1 s:

- On the gas the pitch stays between about 7,600 and 10,400 rpm: 8,300 pulling away from
  slow, rising 1,500 to 2,000 through a gear, dropping 2,000 to 2,500 at the shift, with
  the level dipping for about 0.1 s at each shift. Low revs are never heard on the gas.
- Off the gas (and in the air off the gas) it is the idle take at its own pitch (3,300 to
  3,700), about 14 dB down and dull, within 0.3 s of letting go.
- The gas held with the rear wheel free (the wrecks) is the limiter loop, steady at
  11,500, as loud as riding.

Matching that recording's pitch track (every 0.05 s) against every take's found the
takes themselves, at a pitch ratio of 1.000 (within 0.3 %) and from near their start:
`HIGH_08` at 0.3 s, `LOW_01` at 0.85 s, `POWER_01` at 2.9 s, `HIGH_10` at 6.4 s, `MID_02`
at 8.65 s, `HIGH_04` at 9.85 s, `MID_07` at 10.7 s, `PANIC_START` at 5.7 s and 12.1 s (the
wrecks). So Reflex does not pitch the takes to the engine: at each gear change it starts a
take from the start and lets it run at the pitch and level it was recorded at (the rise
through a gear is the take's own, 10 to 20 %; the idle is quiet because its recording is,
12 dB under the throttle takes). Between two gears' takes the level dips for about 0.1 s.
Which pool each gear uses could not be told from the recording.
