# MX vs ATV Reflex: engine sounds

What `reflex_assets::engine_audio` reads and `halo3_audio::engine` plays. Worked out
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

The event file names, per event, the takes it picks from (the `/MXB_450F_THROTTLE_MID`
string, then the `.wav` names under it). Its parameter curves (rpm and load per layer)
are not decoded, so `engine.rs` does its own mixing from measured pitch: each take's
firing frequency (the spacing of its harmonics, measured on 0.2 s windows; rpm =
frequency × 120 for a four-stroke single) gives:

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

Most throttle takes were recorded accelerating, rising 10 to 20 % in pitch through the
take; `engine.rs` follows each one's pitch through it at load and undoes the rise.
