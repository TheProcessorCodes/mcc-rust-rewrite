# Format probes

The Python probes the format notes cite (here and in `../verify`). They read the maps of
your own MCC install from the same variables the game takes as dev overrides:
`HALO3_MAPS` (`<MCC>\halo3\maps`), `HALO2_MAPS` (`<MCC>\halo2\h2_maps_win64_dx11`),
`HALO1_MAPS` (`<MCC>\halo1\maps`) and `HALOREACH_MAPS` (`<MCC>\haloreach\maps`). Most also
take a map path as their first argument.

```
$env:HALO3_MAPS = '<MCC>\halo3\maps'
python docs\format\scripts\check_winding.py
```
