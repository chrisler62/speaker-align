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

// ─── Interpolation parabolique sub-sample ────────────────────────────────────
//
// Fit une parabole sur 3 points autour du pic pour obtenir une résolution
// ~0.1 sample → ~0.7 mm à 48 kHz.

fn parabolic_interp(y_minus: f32, y_center: f32, y_plus: f32) -> f32 {
    let denom = y_minus - 2.0 * y_center + y_plus;
    if denom.abs() < 1e-12 {
        return 0.0;
    }
    0.5 * (y_minus - y_plus) / denom
}

// ─── GCC-PHAT (Generalized Cross-Correlation — Phase Transform) ─────────────
//
// Corrélation croisée dans le domaine fréquentiel avec normalisation PHAT.
// Produit un pic ultra-net, insensible aux réflexions.

fn gcc_phat(reference: &[f32], test: &[f32], sample_rate: u32) -> f32 {
    let total_len = reference.len() + test.len();
    let fft_len = total_len.next_power_of_two();

    let mut planner = FftPlanner::<f32>::new();
    let fft_fwd = planner.plan_fft_forward(fft_len);
    let fft_inv = planner.plan_fft_inverse(fft_len);

    // Zero-pad reference et test
    let mut ref_buf: Vec<Complex<f32>> = reference
        .iter()
        .map(|&x| Complex::new(x, 0.0))
        .chain(std::iter::repeat_n(Complex::new(0.0, 0.0),fft_len - reference.len()))
        .collect();

    let mut test_buf: Vec<Complex<f32>> = test
        .iter()
        .map(|&x| Complex::new(x, 0.0))
        .chain(std::iter::repeat_n(Complex::new(0.0, 0.0),fft_len - test.len()))
        .collect();

    fft_fwd.process(&mut ref_buf);
    fft_fwd.process(&mut test_buf);

    // Cross-spectre avec normalisation PHAT : R * conj(T) / |R * conj(T)|
    let mut cross: Vec<Complex<f32>> = ref_buf
        .iter()
        .zip(test_buf.iter())
        .map(|(r, t)| {
            let product = r * t.conj();
            let mag = product.norm();
            if mag > 1e-10 { product / mag } else { Complex::new(0.0, 0.0) }
        })
        .collect();

    fft_inv.process(&mut cross);

    // Normaliser la sortie IFFT
    let inv_n = 1.0 / fft_len as f32;
    for c in cross.iter_mut() {
        *c *= inv_n;
    }

    // Chercher le pic dans la plage ±50ms
    let max_lag = ((sample_rate as f32 * 0.05) as usize).min(fft_len / 2);

    let mut best_k: usize = 0;
    let mut best_val = f32::NEG_INFINITY;

    // Lags positifs : indices 0..max_lag
    for (k, c) in cross.iter().enumerate().take(max_lag + 1) {
        if c.re > best_val {
            best_val = c.re;
            best_k = k;
        }
    }
    // Lags négatifs : indices fft_len-max_lag..fft_len
    for (k, c) in cross.iter().enumerate().skip(fft_len - max_lag) {
        if c.re > best_val {
            best_val = c.re;
            best_k = k;
        }
    }

    // Convertir l'index circulaire en lag signé
    let lag = if best_k <= fft_len / 2 {
        best_k as f32
    } else {
        best_k as f32 - fft_len as f32
    };

    // Interpolation parabolique sub-sample
    let k = best_k;
    let prev = cross[(k + fft_len - 1) % fft_len].re;
    let next = cross[(k + 1) % fft_len].re;
    let delta = parabolic_interp(prev, best_val, next);

    (lag + delta) / sample_rate as f32
}

// ─── Distance absolue d'une enceinte par déconvolution sweep ────────────────
//
// Retourne la distance estimée enceinte→micro en mètres.
// La valeur inclut la latence système (buffer DAC+ADC), constante pour les
// deux canaux → la DIFFÉRENCE gauche/droite est acoustiquement juste.
//
// Algorithme :
//   1. IR = FFT(capture) * FFT(inverse_sweep)⁻¹ → réponse impulsionnelle
//   2. Seuil = 10 % du maximum de l'IR sur une fenêtre réaliste
//   3. Premier passage au-dessus du seuil = arrivée du son direct
//   4. Interpolation parabolique sub-sample pour la précision

pub fn compute_speaker_distance(capture: &[f32], sweep: &[f32], sample_rate: u32) -> Option<f32> {
    let sweep_len = sweep.len();
    let total_len = capture.len() + sweep_len;
    let fft_len = total_len.next_power_of_two();

    let mut planner = FftPlanner::<f32>::new();
    let fft_fwd = planner.plan_fft_forward(fft_len);
    let fft_inv = planner.plan_fft_inverse(fft_len);

    // Filtre inverse du sweep log (time-reverse + compensation d'amplitude)
    let f0: f32 = 20.0;
    let f1: f32 = 20_000.0;
    let duration = sweep_len as f32 / sample_rate as f32;
    let rate = (f1 / f0).ln() / duration;

    let inverse_sweep: Vec<f32> = (0..sweep_len)
        .map(|i| {
            let t = (sweep_len - 1 - i) as f32 / sample_rate as f32;
            sweep[sweep_len - 1 - i] * (-rate * t).exp()
        })
        .collect();

    let mut inv_buf: Vec<Complex<f32>> = inverse_sweep
        .iter()
        .map(|&x| Complex::new(x, 0.0))
        .chain(std::iter::repeat_n(Complex::new(0.0, 0.0), fft_len - sweep_len))
        .collect();
    fft_fwd.process(&mut inv_buf);

    let mut cap_buf: Vec<Complex<f32>> = capture
        .iter()
        .map(|&x| Complex::new(x, 0.0))
        .chain(std::iter::repeat_n(Complex::new(0.0, 0.0), fft_len - capture.len()))
        .collect();
    fft_fwd.process(&mut cap_buf);

    let mut ir_buf: Vec<Complex<f32>> = cap_buf
        .iter()
        .zip(inv_buf.iter())
        .map(|(c, h)| c * h)
        .collect();
    fft_inv.process(&mut ir_buf);

    let inv_n = 1.0 / fft_len as f32;

    // La convolution linéaire de capture (N) avec inverse_sweep (M) produit son pic
    // à l'indice (M-1) + t_travel dans l'IR — pas à t_travel directement.
    // Il faut donc décaler la fenêtre de recherche de (sweep_len - 1).
    let offset = sweep_len - 1;

    // Distance maximale réaliste : 20 m → 20/343*48000 ≈ 2800 samples, marge incluse
    let travel_max = (20.0f32 / 343.0 * sample_rate as f32) as usize + 500;
    let search_start = offset;
    let search_end = (offset + travel_max).min(ir_buf.len());

    if search_start >= search_end {
        return None;
    }

    // Extrait la fenêtre [offset .. offset+travel_max] et normalise
    let ir: Vec<f32> = ir_buf[search_start..search_end]
        .iter()
        .map(|c| (c.re * inv_n).abs())
        .collect();

    let max_val = ir.iter().cloned().fold(0.0f32, f32::max);
    if max_val < 1e-9 {
        return None;
    }

    let threshold = max_val * 0.10;

    // Premier passage au-dessus du seuil = front du son direct
    let onset = ir.iter().position(|&v| v >= threshold)?;

    // Pic local dans les 50 samples suivant le front (son direct, avant les réflexions)
    let window_end = (onset + 50).min(ir.len() - 1);
    let peak_idx = (onset..=window_end)
        .max_by(|&a, &b| ir[a].partial_cmp(&ir[b]).unwrap())?;

    // peak_idx est l'indice DANS la fenêtre décalée → directement le temps de trajet
    let delta = if peak_idx > 0 && peak_idx < ir.len() - 1 {
        parabolic_interp(ir[peak_idx - 1], ir[peak_idx], ir[peak_idx + 1])
    } else {
        0.0
    };

    let time_s = (peak_idx as f32 + delta) / sample_rate as f32;
    Some(time_s * 343.0) // distance en mètres
}

// ─── Mesure de délai haute précision (~0.7 mm) ──────────────────────────────
//
// Si les signaux de test (sweep) sont fournis → déconvolution + interp parabolique
// Sinon (bruit rose) → GCC-PHAT + interp parabolique

pub fn compute_delay_precise(
    left_capture: &[f32],
    right_capture: &[f32],
    sample_rate: u32,
    _left_signal: Option<&[f32]>,
    _right_signal: Option<&[f32]>,
) -> f32 {
    // GCC-PHAT direct entre les deux captures pour les deux modes.
    //
    // Mode sweep : generate_sweep() est déterministe → les deux captures
    //   contiennent le même sweep filtré par des chemins acoustiques différents
    //   → GCC-PHAT trouve le délai inter-canal directement.
    //
    // Mode bruit rose : les deux signaux sont des réalisations différentes
    //   (rand non corrélé) mais la corrélation via l'acoustique de la pièce
    //   reste utilisable pour estimer un délai approximatif.
    //
    // La déconvolution séparée canal par canal n'est pas utilisée ici car elle
    // compare des temps absolus de captures séquentielles, ce qui donne des
    // résultats instables (dépend du pic trouvé dans l'IR de chaque canal).
    gcc_phat(left_capture, right_capture, sample_rate)
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
