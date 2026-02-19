// ============================================================
//  dsp.rs — Traitement du signal audio
//
//  - FFT via rustfft (O(n log n), fenêtre de Hann)
//  - Découpage log en bandes (20 Hz – 20 kHz)
//  - RMS, corrélation croisée pour le délai
//  - Score global (fréquence + niveau + temps)
// ============================================================

use rustfft::{FftPlanner, num_complex::Complex};
use std::f32::consts::PI;

pub const SAMPLE_RATE: u32 = 48_000;
pub const FFT_SIZE: usize = 8_192;
pub const NUM_BANDS: usize = 128;
pub const SWEEP_DURATION: f32 = 3.0;
pub const CAPTURE_DURATION: f32 = 4.0;

// ─── Génération du sweep sinusoïdal logarithmique ────────────────────────────

pub fn generate_sweep(sample_rate: u32, duration: f32) -> Vec<f32> {
    let len = (duration * sample_rate as f32) as usize;
    let f0: f32 = 20.0;
    let f1: f32 = 20_000.0;
    let k = f1 / f0;
    let mut buf = Vec::with_capacity(len);

    for i in 0..len {
        let t = i as f32 / sample_rate as f32;
        let phase = 2.0 * PI * f0 * duration / k.ln() * (k.powf(t / duration) - 1.0);
        let env = (t * 20.0).min(1.0) * ((duration - t) * 20.0).min(1.0);
        buf.push(phase.sin() * 0.7 * env);
    }
    buf
}

// ─── Génération du bruit rose (algorithme de Voss-McCartney) ─────────────────

pub fn generate_pink_noise(sample_rate: u32, duration: f32) -> Vec<f32> {
    let len = (duration * sample_rate as f32) as usize;
    let (mut b0, mut b1, mut b2, mut b3, mut b4, mut b5, mut b6) =
        (0f32, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0);
    let mut buf = Vec::with_capacity(len);

    for i in 0..len {
        let white: f32 = rand::random::<f32>() * 2.0 - 1.0;
        b0 = 0.99886 * b0 + white * 0.0555179;
        b1 = 0.99332 * b1 + white * 0.0750759;
        b2 = 0.96900 * b2 + white * 0.1538520;
        b3 = 0.86650 * b3 + white * 0.3104856;
        b4 = 0.55000 * b4 + white * 0.5329522;
        b5 = -0.7616 * b5 - white * 0.0168980;
        let pink = b0 + b1 + b2 + b3 + b4 + b5 + b6 + white * 0.5362;
        b6 = white * 0.115926;
        let t = i as f32 / sample_rate as f32;
        let env = (t * 10.0).min(1.0) * ((duration - t) * 10.0).min(1.0);
        buf.push(pink * 0.06 * env);
    }
    buf
}

// ─── FFT avec fenêtre de Hann, moyennée sur les segments ─────────────────────

pub fn compute_fft(samples: &[f32]) -> Vec<f32> {
    let n = FFT_SIZE;
    let half = n / 2;
    let num_segments = samples.len() / n;

    if num_segments == 0 {
        return vec![0.0; half];
    }

    let mut planner = FftPlanner::<f32>::new();
    let fft = planner.plan_fft_forward(n);

    // Fenêtre de Hann précalculée
    let window: Vec<f32> = (0..n)
        .map(|i| 0.5 * (1.0 - (2.0 * PI * i as f32 / (n - 1) as f32).cos()))
        .collect();

    let mut spectrum = vec![0.0f32; half];

    for seg in 0..num_segments {
        let offset = seg * n;
        let mut buf: Vec<Complex<f32>> = (0..n)
            .map(|i| Complex::new(samples[offset + i] * window[i], 0.0))
            .collect();

        fft.process(&mut buf);

        for k in 0..half {
            let mag = buf[k].norm() / n as f32;
            spectrum[k] += mag;
        }
    }

    // Moyenne sur les segments
    for v in spectrum.iter_mut() {
        *v /= num_segments as f32;
    }

    spectrum
}

// ─── Découpage du spectre en bandes logarithmiques ───────────────────────────

pub fn spectrum_to_bands(spectrum: &[f32], sample_rate: u32, num_bands: usize) -> Vec<f32> {
    let freq_res = sample_rate as f32 / FFT_SIZE as f32;
    let log_min = 20f32.log10();
    let log_max = 20_000f32.log10();
    let mut bands = vec![0.0f32; num_bands];

    for b in 0..num_bands {
        let f0 = 10f32.powf(log_min + (log_max - log_min) * b as f32 / num_bands as f32);
        let f1 = 10f32.powf(log_min + (log_max - log_min) * (b + 1) as f32 / num_bands as f32);
        let k0 = ((f0 / freq_res) as usize).max(1);
        let k1 = ((f1 / freq_res).ceil() as usize).min(spectrum.len() - 1);

        let (mut sum, mut count) = (0.0f32, 0usize);
        for k in k0..=k1 {
            sum += spectrum[k];
            count += 1;
        }
        bands[b] = if count > 0 { sum / count as f32 } else { 0.0 };
    }
    bands
}

// ─── Conversion en dB ────────────────────────────────────────────────────────

pub fn bands_to_db(bands: &[f32]) -> Vec<f32> {
    bands
        .iter()
        .map(|&v| if v > 0.0 { 20.0 * v.log10() } else { -100.0 })
        .collect()
}

// ─── Filtre passe-haut (IIR 1er ordre) ───────────────────────────────────────
//
// Élimine le bruit de ronflement ambiant (ventilateurs, vibrations sol/bureau)
// sans affecter la plage utile des enceintes (> 80 Hz).
// Cutoff par défaut : 30 Hz.

pub fn highpass_filter(samples: &[f32], cutoff_hz: f32, sample_rate: u32) -> Vec<f32> {
    let alpha = 1.0 / (1.0 + 2.0 * PI * cutoff_hz / sample_rate as f32);
    let mut out = Vec::with_capacity(samples.len());
    let mut prev_in = 0.0f32;
    let mut prev_out = 0.0f32;

    for &x in samples {
        let y = alpha * (prev_out + x - prev_in);
        prev_in = x;
        prev_out = y;
        out.push(y);
    }
    out
}

// ─── RMS ─────────────────────────────────────────────────────────────────────

pub fn compute_rms(samples: &[f32]) -> f32 {
    let sum: f32 = samples.iter().map(|x| x * x).sum();
    (sum / samples.len() as f32).sqrt()
}

// ─── Corrélation croisée pour estimer le délai entre les deux canaux ─────────

pub fn compute_delay(reference: &[f32], test: &[f32], sample_rate: u32) -> f32 {
    let max_lag = ((sample_rate as f32 * 0.05) as usize)
        .min(reference.len())
        .min(test.len());

    let mut best_lag: i32 = 0;
    let mut best_corr = f32::NEG_INFINITY;

    for lag in -(max_lag as i32)..=(max_lag as i32) {
        let n = (reference.len().min(test.len()) as i32 - lag.abs()) as usize;
        if n == 0 {
            continue;
        }

        let corr: f32 = (0..n)
            .map(|i| {
                let ri = if lag >= 0 { i } else { (i as i32 - lag) as usize };
                let ti = if lag >= 0 { (i as i32 + lag) as usize } else { i };
                if ri < reference.len() && ti < test.len() {
                    reference[ri] * test[ti]
                } else {
                    0.0
                }
            })
            .sum();

        if corr > best_corr {
            best_corr = corr;
            best_lag = lag;
        }
    }

    best_lag as f32 / sample_rate as f32
}

// ─── Score global (0–100) ─────────────────────────────────────────────────────

pub fn compute_score(
    left_db: &[f32],
    right_db: &[f32],
    delay_ms: f32,
    level_diff_db: f32,
) -> u32 {
    // Similarité spectrale → 0-50 pts
    let freq_error: f32 = left_db
        .iter()
        .zip(right_db.iter())
        .map(|(l, r)| (l - r).abs())
        .sum::<f32>()
        / left_db.len() as f32;
    let freq_score = (50.0 - freq_error * 2.0).max(0.0);

    // Équilibre de niveau → 0-25 pts
    let level_score = (25.0 - level_diff_db.abs() * 5.0).max(0.0);

    // Alignement temporel → 0-25 pts
    let time_score = (25.0 - delay_ms.abs() * 10.0).max(0.0);

    (freq_score + level_score + time_score).round() as u32
}

// ─── Inclinaison spectrale ────────────────────────────────────────────────────

pub fn compute_freq_tilt(left_db: &[f32], right_db: &[f32]) -> f32 {
    let mid = NUM_BANDS / 2;

    let avg = |slice: &[f32]| -> f32 {
        slice.iter().sum::<f32>() / slice.len() as f32
    };

    let left_low = avg(&left_db[..mid]);
    let left_high = avg(&left_db[mid..]);
    let right_low = avg(&right_db[..mid]);
    let right_high = avg(&right_db[mid..]);

    (right_high - right_low) - (left_high - left_low)
}

// ─── Fréquence centrale d'une bande ──────────────────────────────────────────

pub fn band_center_freq(index: usize, num_bands: usize) -> f32 {
    let log_min = 20f32.log10();
    let log_max = 20_000f32.log10();
    10f32.powf(log_min + (log_max - log_min) * (index as f32 + 0.5) / num_bands as f32)
}

pub fn freq_label(index: usize, num_bands: usize) -> String {
    let f = band_center_freq(index, num_bands);
    if f >= 1000.0 {
        format!("{:.0}k", f / 1000.0)
    } else {
        format!("{:.0}", f)
    }
}
