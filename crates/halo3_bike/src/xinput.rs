//! The triggers read straight from XInput, the Xbox pad's own Windows API.
//!
//! bevy reads pads through gilrs and Windows.Gaming.Input. That path is
//! analog, but reading XInput as well takes every layer between the driver
//! and the bike out of the question: these are the bytes the driver holds,
//! 0 to 255, the same ones XInput games read. Not on Windows, or with no
//! XInput pad connected, `triggers` gives `None` and the bike falls back to
//! bevy's reading.

/// Left and right trigger, 0 to 1, of the first connected XInput pad.
#[cfg(windows)]
pub fn triggers() -> Option<[f32; 2]> {
    use std::sync::Mutex;
    use std::time::{Duration, Instant};

    #[repr(C)]
    #[derive(Default)]
    struct Gamepad {
        buttons: u16,
        left_trigger: u8,
        right_trigger: u8,
        thumbs: [i16; 4],
    }
    #[repr(C)]
    #[derive(Default)]
    struct State {
        packet: u32,
        pad: Gamepad,
    }
    #[link(name = "xinput")]
    unsafe extern "system" {
        fn XInputGetState(user: u32, state: *mut State) -> u32;
    }
    let read = |user: u32| {
        let mut s = State::default();
        // SAFETY: `s` is a valid XINPUT_STATE for the call to fill.
        (unsafe { XInputGetState(user, &mut s) } == 0)
            .then(|| [s.pad.left_trigger as f32 / 255.0, s.pad.right_trigger as f32 / 255.0])
    };

    // Asking an empty slot is slow (it can take a millisecond or more), so
    // keep to the slot that answered and look for a pad once a second.
    static FOUND: Mutex<(Option<u32>, Option<Instant>)> = Mutex::new((None, None));
    let mut found = FOUND.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(user) = found.0 {
        if let Some(t) = read(user) {
            return Some(t);
        }
        found.0 = None;
    }
    if found.1.is_some_and(|t| t.elapsed() < Duration::from_secs(1)) {
        return None;
    }
    found.1 = Some(Instant::now());
    let user = (0..4).find(|&u| read(u).is_some())?;
    found.0 = Some(user);
    read(user)
}

#[cfg(not(windows))]
pub fn triggers() -> Option<[f32; 2]> {
    None
}
