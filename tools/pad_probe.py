#!/usr/bin/env python3
r"""pad_probe.py: show what Windows reports for a gamepad's triggers, without the game.

The bike reads the right trigger through bevy and gilrs, which take it from the Xbox
driver. This reads the same driver through XInput and prints the raw trigger byte
(0..255) as you move it, so you can tell a pad or profile that sends the trigger as
on/off from a game that misreads it.

Run (standard library only; reads the pad, writes nothing):
  python -I tools\pad_probe.py [--seconds 20]

Hold RT at a few different depths for a second each. The last line says whether
values between 0 and 255 were seen.
"""
import argparse
import ctypes
import sys
import time

sys.dont_write_bytecode = True


class XINPUT_GAMEPAD(ctypes.Structure):
    _fields_ = [
        ('wButtons', ctypes.c_ushort),
        ('bLeftTrigger', ctypes.c_ubyte),
        ('bRightTrigger', ctypes.c_ubyte),
        ('sThumbLX', ctypes.c_short),
        ('sThumbLY', ctypes.c_short),
        ('sThumbRX', ctypes.c_short),
        ('sThumbRY', ctypes.c_short),
    ]


class XINPUT_STATE(ctypes.Structure):
    _fields_ = [('dwPacketNumber', ctypes.c_uint), ('Gamepad', XINPUT_GAMEPAD)]


def load_xinput():
    for name in ('xinput1_4', 'xinput1_3', 'xinput9_1_0'):
        try:
            return ctypes.WinDLL(name), name
        except OSError:
            continue
    return None, None


def bar(v, width=25):
    n = int(round(v * width))
    return '[' + '#' * n + '.' * (width - n) + ']'


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.split('\n')[0])
    ap.add_argument('--seconds', type=float, default=20.0)
    a = ap.parse_args(argv)
    dll, name = load_xinput()
    if dll is None:
        print('XInput DLL not found (xinput1_4 / xinput1_3).')
        return 2
    get = dll.XInputGetState
    get.argtypes = [ctypes.c_uint, ctypes.POINTER(XINPUT_STATE)]
    get.restype = ctypes.c_uint
    st = XINPUT_STATE()
    pads = [i for i in range(4) if get(i, ctypes.byref(st)) == 0]
    print('XInput: %s; connected pads (user index): %s' % (name, pads or 'none'))
    if not pads:
        print('No XInput pad connected. RESULT: NO PAD')
        return 1
    print('Hold RT at different depths for a second each; LT too if you like.')
    t_end = time.time() + a.seconds
    seen = set()
    last = None
    while time.time() < t_end:
        parts = []
        for i in pads:
            get(i, ctypes.byref(st))
            rt, lt = st.Gamepad.bRightTrigger, st.Gamepad.bLeftTrigger
            seen.add(rt)
            parts.append('pad %d  RT %3d %s  LT %3d %s' % (i, rt, bar(rt / 255.0), lt, bar(lt / 255.0)))
        line = '   '.join(parts)
        if line != last:
            print(line)
            last = line
        time.sleep(0.02)
    mid = sorted(v for v in seen if 20 <= v <= 235)
    print()
    print('Distinct RT bytes seen: %d; between 20 and 235: %d %s' % (len(seen), len(mid), mid[:40]))
    if not seen:
        print('RESULT: NO SAMPLES (run for longer)')
    elif len(mid) >= 5:
        print('RESULT: ANALOG')
    else:
        print('RESULT: ONLY 0/255 (the pad or its profile sends the trigger as on/off, or it was not moved slowly)')
    return 0


if __name__ == '__main__':
    sys.exit(main())
