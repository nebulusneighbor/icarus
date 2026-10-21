//! Measurements that describe audio for a model that cannot hear it directly.

use rustfft::{FftPlanner, num_complex::Complex};
use serde::Serialize;

/// Floor for every dB value, so silence stays a finite number in JSON.
pub const FLOOR_DB: f64 = -120.0;

const FFT_SIZE: usize = 2048;
const LOW_MID_HZ: f64 = 250.0;
const MID_HIGH_HZ: f64 = 4_000.0;

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize)]
pub struct Features {
    /// RMS level of the mid (mono) signal, dBFS.
    pub rms_db: f64,
    /// Sample peak across both channels, dBFS.
    pub peak_db: f64,
    /// Peak minus RMS. High values mean spiky, transient material; low values, dense or squashed.
    pub crest_db: f64,
    /// Magnitude-weighted spectral centroid in Hz: a rough measure of brightness.
    pub centroid_hz: f64,
    /// Share of spectral energy below 250 Hz, dB relative to the total.
    pub low_db: f64,
    /// Share of spectral energy between 250 Hz and 4 kHz, dB relative to the total.
    pub mid_db: f64,
    /// Share of spectral energy above 4 kHz, dB relative to the total.
    pub high_db: f64,
    /// Side RMS over mid RMS. 0 is mono; around 1 is very wide.
    pub width: f64,
}

impl Features {
    pub const NAMES: [&'static str; 8] = [
        "rms_db",
        "peak_db",
        "crest_db",
        "centroid_hz",
        "low_db",
        "mid_db",
        "high_db",
        "width",
    ];

    pub fn values(&self) -> [f64; 8] {
        [
            self.rms_db,
            self.peak_db,
            self.crest_db,
            self.centroid_hz,
            self.low_db,
            self.mid_db,
            self.high_db,
            self.width,
        ]
    }
}

fn to_db(x: f64) -> f64 {
    if x > 0.0 {
        (20.0 * x.log10()).max(FLOOR_DB)
    } else {
        FLOOR_DB
    }
}

fn power_to_db(x: f64) -> f64 {
    if x > 0.0 {
        (10.0 * x.log10()).max(FLOOR_DB)
    } else {
        FLOOR_DB
    }
}

pub fn analyze(l: &[f32], r: &[f32], sample_rate: u32) -> Features {
    let n = l.len().min(r.len());
    if n == 0 {
        return Features {
            rms_db: FLOOR_DB,
            peak_db: FLOOR_DB,
            low_db: FLOOR_DB,
            mid_db: FLOOR_DB,
            high_db: FLOOR_DB,
            ..Features::default()
        };
    }

    let mid: Vec<f64> = (0..n).map(|i| (f64::from(l[i]) + f64::from(r[i])) * 0.5).collect();
    let side_sq: f64 = (0..n)
        .map(|i| ((f64::from(l[i]) - f64::from(r[i])) * 0.5).powi(2))
        .sum();
    let mid_sq: f64 = mid.iter().map(|m| m * m).sum();
    let peak = l[..n].iter().chain(&r[..n]).fold(0f32, |a, &b| a.max(b.abs()));

    let rms = (mid_sq / n as f64).sqrt();
    let rms_db = to_db(rms);
    let peak_db = to_db(f64::from(peak));

    let spectrum = average_power_spectrum(&mid);
    let bin_hz = f64::from(sample_rate) / FFT_SIZE as f64;
    let (mut total, mut magnitude, mut weighted) = (0.0, 0.0, 0.0);
    let (mut low, mut midband, mut high) = (0.0, 0.0, 0.0);
    for (bin, &p) in spectrum.iter().enumerate().skip(1) {
        let hz = bin as f64 * bin_hz;
        total += p;
        magnitude += p.sqrt();
        weighted += hz * p.sqrt();
        match hz {
            h if h < LOW_MID_HZ => low += p,
            h if h < MID_HIGH_HZ => midband += p,
            _ => high += p,
        }
    }
    let share = |band: f64| {
        if total > 0.0 {
            power_to_db(band / total)
        } else {
            FLOOR_DB
        }
    };

    Features {
        rms_db,
        peak_db,
        crest_db: peak_db - rms_db,
        centroid_hz: if magnitude > 0.0 { weighted / magnitude } else { 0.0 },
        low_db: share(low),
        mid_db: share(midband),
        high_db: share(high),
        width: if mid_sq > 0.0 { (side_sq / mid_sq).sqrt() } else { 0.0 },
    }
}

/// Hann-windowed, 50%-overlapped frames averaged into one power spectrum.
fn average_power_spectrum(signal: &[f64]) -> Vec<f64> {
    let fft = FftPlanner::<f64>::new().plan_fft_forward(FFT_SIZE);
    let window: Vec<f64> = (0..FFT_SIZE)
        .map(|i| 0.5 - 0.5 * (std::f64::consts::TAU * i as f64 / FFT_SIZE as f64).cos())
        .collect();
    let mut acc = vec![0.0; FFT_SIZE / 2 + 1];
    let mut frames = 0;
    let mut start = 0;
    let mut buf = vec![Complex::new(0.0, 0.0); FFT_SIZE];
    loop {
        for (i, slot) in buf.iter_mut().enumerate() {
            let x = signal.get(start + i).copied().unwrap_or(0.0);
            *slot = Complex::new(x * window[i], 0.0);
        }
        fft.process(&mut buf);
        for (a, c) in acc.iter_mut().zip(&buf) {
            *a += c.norm_sqr();
        }
        frames += 1;
        start += FFT_SIZE / 2;
        if start + FFT_SIZE > signal.len() {
            break;
        }
    }
    acc.iter_mut().for_each(|a| *a /= f64::from(frames));
    acc
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(hz: f32, n: usize) -> Vec<f32> {
        (0..n)
            .map(|i| (std::f32::consts::TAU * hz * i as f32 / 48_000.0).sin())
            .collect()
    }

    #[test]
    fn sine_levels_and_centroid() {
        let s = sine(1_000.0, 48_000);
        let f = analyze(&s, &s, 48_000);
        assert!((f.peak_db - 0.0).abs() < 0.01);
        assert!((f.rms_db + 3.01).abs() < 0.05);
        assert!((f.centroid_hz - 1_000.0).abs() < 30.0, "centroid {}", f.centroid_hz);
        assert!(f.mid_db > -0.1 && f.width < 1e-9);
    }

    #[test]
    fn silence_is_finite() {
        let z = vec![0.0; 4_800];
        let f = analyze(&z, &z, 48_000);
        assert_eq!(f.rms_db, FLOOR_DB);
        assert!(f.values().iter().all(|v| v.is_finite()));
    }
}
