//! Measures the synth against Reflex's 450F recordings, to tune the synth
//! by numbers rather than by ear: the spectrum in third-octave bands, each
//! harmonic of the firing frequency, how much is noise between them, and
//! how much one firing differs from the next. Only these summaries are
//! printed; nothing of the recordings is kept.
//!
//! `cargo test -p halo3_audio engine_measure -- --ignored --nocapture`
//! (needs Reflex set up).

use std::sync::{Arc, Mutex};

use crate::engine::{Input, Synth};
use crate::mixer::{Generator, RATE};

const N: usize = 8192;

fn fft(re: &mut [f64], im: &mut [f64]) {
    let n = re.len();
    let mut j = 0;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j |= bit;
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
    }
    let mut len = 2;
    while len <= n {
        let a = -std::f64::consts::TAU / len as f64;
        for start in (0..n).step_by(len) {
            for k in 0..len / 2 {
                let (s, c) = (a * k as f64).sin_cos();
                let (i0, i1) = (start + k, start + k + len / 2);
                let (xr, xi) = (re[i1] * c - im[i1] * s, re[i1] * s + im[i1] * c);
                re[i1] = re[i0] - xr;
                im[i1] = im[i0] - xi;
                re[i0] += xr;
                im[i0] += xi;
            }
        }
        len <<= 1;
    }
}

/// Welch power spectrum, Hann windows of `N`, half overlap.
fn power(x: &[f32]) -> Vec<f64> {
    let mut sum = vec![0.0; N / 2];
    let mut count = 0;
    let mut at = 0;
    while at + N <= x.len() {
        let mut re: Vec<f64> = (0..N)
            .map(|i| x[at + i] as f64 * (0.5 - 0.5 * (std::f64::consts::TAU * i as f64 / N as f64).cos()))
            .collect();
        let mut im = vec![0.0; N];
        fft(&mut re, &mut im);
        for k in 0..N / 2 {
            sum[k] += re[k] * re[k] + im[k] * im[k];
        }
        count += 1;
        at += N / 2;
    }
    sum.iter().map(|s| s / count.max(1) as f64).collect()
}

fn db(x: f64) -> f64 {
    10.0 * x.max(1e-20).log10()
}

struct Summary {
    f0: f64,
    rms_db: f64,
    bands: Vec<f64>,
    harmonics: Vec<f64>,
    noise_db: f64,
    spread: f64,
}

const BANDS: std::ops::RangeInclusive<i32> = -12..=10;

fn summarise(x: &[f32], rate: f64, lo: f64, hi: f64) -> Summary {
    let p = power(x);
    let bin = rate / N as f64;
    let peak_near = |f: f64| -> f64 {
        let c = (f / bin).round() as isize;
        (c - 1..=c + 1).filter(|&k| k > 0 && (k as usize) < p.len()).map(|k| p[k as usize]).fold(0.0, f64::max)
    };
    // The firing frequency: the comb that gathers most.
    let mut best = (lo, 0.0);
    let mut f = lo;
    while f <= hi {
        let e: f64 = (1..=24).map(|k| peak_near(f * k as f64).sqrt()).sum();
        if e > best.1 {
            best = (f, e);
        }
        f += 0.05;
    }
    let f0 = best.0;
    let total: f64 = p.iter().sum();
    let bands = BANDS
        .map(|k| {
            let c = 1000.0 * 2f64.powf(k as f64 / 3.0);
            let (a, b) = (c / 2f64.powf(1.0 / 6.0), c * 2f64.powf(1.0 / 6.0));
            let e: f64 = p.iter().enumerate().filter(|(i, _)| (a..b).contains(&(*i as f64 * bin))).map(|(_, v)| v).sum();
            db(e / total)
        })
        .collect();
    let h: Vec<f64> = (1..=16).map(|k| peak_near(f0 * k as f64)).collect();
    let top = h.iter().cloned().fold(0.0, f64::max);
    let harmonics = h.iter().map(|v| db(v / top)).collect();
    // Noise: what lies off the harmonics up to 5 kHz.
    let mut on = 0.0;
    let mut all = 0.0;
    for (i, v) in p.iter().enumerate() {
        let fr = i as f64 * bin;
        if !(20.0..5000.0).contains(&fr) {
            continue;
        }
        all += v;
        let k = (fr / f0).round();
        if k >= 1.0 && (fr - k * f0).abs() < 1.6 * bin {
            on += v;
        }
    }
    // Firing to firing: each cycle's rms.
    let cycle = (rate / f0) as usize;
    let rms: Vec<f64> = x
        .chunks(cycle.max(1))
        .filter(|c| c.len() == cycle)
        .map(|c| (c.iter().map(|v| (*v as f64).powi(2)).sum::<f64>() / c.len() as f64).sqrt())
        .collect();
    let mean = rms.iter().sum::<f64>() / rms.len().max(1) as f64;
    let var = rms.iter().map(|r| (r - mean).powi(2)).sum::<f64>() / rms.len().max(1) as f64;
    let level = (x.iter().map(|v| (*v as f64).powi(2)).sum::<f64>() / x.len().max(1) as f64).sqrt();
    Summary {
        f0,
        rms_db: 20.0 * level.max(1e-12).log10(),
        bands,
        harmonics,
        noise_db: db((all - on) / all.max(1e-20)),
        spread: var.sqrt() / mean.max(1e-12),
    }
}

fn print(name: &str, s: &Summary) {
    println!(
        "{name:<34} f0 {:6.2} Hz ({:5.0} rpm)  rms {:6.1} dB  noise {:5.1} dB  spread {:.3}",
        s.f0,
        s.f0 * 120.0,
        s.rms_db,
        s.noise_db,
        s.spread
    );
    println!("  bands  {}", s.bands.iter().map(|v| format!("{:4.0}", v)).collect::<Vec<_>>().join(""));
    println!("  harm   {}", s.harmonics.iter().map(|v| format!("{:4.0}", v)).collect::<Vec<_>>().join(""));
}

/// The synth held at `rpm` and `throttle`, mono, after it settles.
pub(crate) fn synth_steady(rpm: f32, throttle: f32, speed: f32, seconds: f32) -> Vec<f32> {
    let shared = Arc::new(Mutex::new(Input {
        rpm,
        min_rpm: 1500.0,
        max_rpm: 11500.0,
        throttle,
        gear: 2,
        speed,
        volume: 1.0,
        ..Input::default()
    }));
    let mut s = Synth::new(shared);
    let mut out = Vec::new();
    let mut buf = vec![0.0; 2048];
    let blocks = ((seconds + 0.5) * RATE as f32 / 1024.0) as usize;
    for n in 0..blocks {
        buf.fill(0.0);
        s.render(&mut buf);
        if n as f32 * 1024.0 / RATE as f32 >= 0.5 {
            out.extend(buf.chunks(2).map(|c| c[0]));
        }
    }
    out
}

#[test]
#[ignore]
fn engine_measure() {
    let Some(root) = reflex_assets::find_reflex() else {
        eprintln!("no Reflex set up; skipped");
        return;
    };
    let engine = reflex_assets::engine_audio::load_450f(&root).unwrap();
    let bands: Vec<String> = BANDS.map(|k| format!("{:4.0}", 1000.0 * 2f64.powf(k as f64 / 3.0))).collect();
    println!("band centres (Hz): {}", bands.join(""));
    // Steady takes: (pool, firing range Hz, synth throttle).
    let cases = [
        ("MXB_450F_IDLE", 20.0, 40.0, 0.0),
        ("MXB_450F_HALF_THROTTLE", 24.0, 45.0, 0.3),
        ("MXB_450F_PANIC_LOOP", 85.0, 100.0, 1.0),
    ];
    for (def, lo, hi, gas) in cases {
        let Some(d) = engine.defs.iter().find(|d| d.name == def) else {
            continue;
        };
        let mut f0s = Vec::new();
        for take in &d.takes {
            let Some(c) = engine.clips.iter().find(|c| &c.name == take) else {
                continue;
            };
            let n = c.samples.len();
            let mid = &c.samples[n / 5..n * 4 / 5];
            if mid.len() < N * 2 {
                continue;
            }
            let s = summarise(mid, c.rate as f64, lo, hi);
            print(&c.name, &s);
            f0s.push(s.f0);
        }
        if f0s.is_empty() {
            continue;
        }
        let f0 = f0s.iter().sum::<f64>() / f0s.len() as f64;
        let x = synth_steady((f0 * 120.0) as f32, gas, 0.0, 4.0);
        print(&format!("synth at {:.0} rpm, gas {gas}", f0 * 120.0), &summarise(&x, RATE as f64, lo, hi));
        println!();
    }
    // A throttle take for its spread over the climb: the band shape only.
    for def in ["MXB_450F_THROTTLE_MID", "MXB_450F_THROTTLE_HIGH"] {
        if let Some(d) = engine.defs.iter().find(|d| d.name == def) {
            for take in d.takes.iter().take(2) {
                if let Some(c) = engine.clips.iter().find(|c| &c.name == take) {
                    let s = summarise(&c.samples, c.rate as f64, 55.0, 90.0);
                    print(&c.name, &s);
                }
            }
        }
    }
    let x = synth_steady(8500.0, 1.0, 0.0, 3.0);
    print("synth at 8500 rpm, gas 1", &summarise(&x, RATE as f64, 55.0, 90.0));
}
