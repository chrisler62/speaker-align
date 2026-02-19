// ============================================================
//  audio.rs — Lecture & capture audio via cpal
//
//  - Lecture d'un signal de test sur le canal gauche ou droit
//  - Enregistrement simultané depuis le microphone
//  - Support : WASAPI (Windows), CoreAudio (macOS), ALSA (Linux)
// ============================================================

use anyhow::{Context, Result, bail};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, SampleRate, StreamConfig};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::dsp::SAMPLE_RATE;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Channel {
    Left,
    Right,
}

/// Lance la lecture du signal `signal` sur le canal choisi,
/// et capture simultanément le microphone pendant `capture_secs` secondes.
/// `pre_delay_secs` : pause silencieuse avant le démarrage (évite d'enregistrer la frappe clavier).
/// Retourne les échantillons capturés (mono f32, taux = SAMPLE_RATE).
pub fn play_and_capture(
    signal: &[f32],
    channel: Channel,
    capture_secs: f32,
    pre_delay_secs: f32,
    progress_tx: std::sync::mpsc::Sender<f32>,
) -> Result<Vec<f32>> {
    let host = cpal::default_host();

    // ── Sortie ──────────────────────────────────────────────────────────────
    let output_device = host
        .default_output_device()
        .context("Aucune sortie audio disponible")?;

    let out_config = find_stereo_config(&output_device, SampleRate(SAMPLE_RATE))
        .context("Format de sortie stéréo 48 kHz introuvable")?;

    // Prépare le buffer de lecture multicanal (interleaved, signal sur ch0 ou ch1, zéros ailleurs)
    let num_out_channels = out_config.channels as usize;
    let play_buf: Arc<Vec<f32>> = Arc::new(interleave_to_multichannel(signal, channel, num_out_channels));
    let play_pos = Arc::new(Mutex::new(0usize));

    let pb = Arc::clone(&play_buf);
    let pp = Arc::clone(&play_pos);

    let out_stream = output_device.build_output_stream(
        &out_config,
        move |data: &mut [f32], _| {
            let mut pos = pp.lock().unwrap();
            for frame in data.chunks_mut(num_out_channels) {
                if *pos + num_out_channels <= pb.len() {
                    frame.copy_from_slice(&pb[*pos..*pos + num_out_channels]);
                    *pos += num_out_channels;
                } else {
                    for s in frame.iter_mut() {
                        *s = 0.0;
                    }
                }
            }
        },
        |e| eprintln!("Erreur sortie audio : {}", e),
        None,
    )?;

    // ── Entrée ──────────────────────────────────────────────────────────────
    let input_device = host
        .default_input_device()
        .context("Aucun microphone disponible. Branchez un micro et réessayez.")?;

    let in_config = find_mono_input_config(&input_device, SampleRate(SAMPLE_RATE))
        .context("Format d'entrée mono 48 kHz introuvable")?;

    let captured: Arc<Mutex<Vec<f32>>> = Arc::new(Mutex::new(Vec::new()));
    let cap_clone = Arc::clone(&captured);

    let in_stream = input_device.build_input_stream(
        &in_config,
        move |data: &[f32], _| {
            let mut buf = cap_clone.lock().unwrap();
            // Mix multicanal → mono
            let channels = in_config.channels as usize;
            for frame in data.chunks(channels) {
                let mono = frame.iter().sum::<f32>() / channels as f32;
                buf.push(mono);
            }
        },
        |e| eprintln!("Erreur entrée audio : {}", e),
        None,
    )?;

    // ── Synchronisation ─────────────────────────────────────────────────────
    // Pause avant démarrage pour laisser le bruit de frappe se dissiper
    if pre_delay_secs > 0.0 {
        std::thread::sleep(Duration::from_secs_f32(pre_delay_secs));
    }

    out_stream.play()?;
    in_stream.play()?;

    let total_ms = (capture_secs * 1000.0) as u64;
    let step_ms = 50u64;
    let mut elapsed = 0u64;

    while elapsed < total_ms {
        std::thread::sleep(Duration::from_millis(step_ms));
        elapsed += step_ms;
        let _ = progress_tx.send(elapsed as f32 / total_ms as f32);
    }

    drop(out_stream);
    drop(in_stream);

    let samples = Arc::try_unwrap(captured)
        .unwrap()
        .into_inner()
        .unwrap();

    if samples.is_empty() {
        bail!("Aucun échantillon capturé. Vérifiez que le microphone est actif.");
    }

    Ok(samples)
}

// ─── Utilitaires internes ─────────────────────────────────────────────────────

/// Convertit un signal mono en buffer multicanal interleaved.
/// Le signal est placé sur ch0 (Left) ou ch1 (Right) ; tous les autres canaux
/// (centre, LFE, surround…) restent à zéro.
fn interleave_to_multichannel(mono: &[f32], channel: Channel, num_channels: usize) -> Vec<f32> {
    let ch_idx = match channel {
        Channel::Left  => 0,
        Channel::Right => 1.min(num_channels - 1),
    };
    let mut out = vec![0.0f32; mono.len() * num_channels];
    for (i, &s) in mono.iter().enumerate() {
        out[i * num_channels + ch_idx] = s;
    }
    out
}

/// Cherche une config de sortie à 48 kHz — préfère la stéréo, accepte 5.1/7.1.
/// Le signal sera toujours routé sur FL (ch0) et FR (ch1), les canaux
/// supplémentaires étant mis à zéro, ce qui fonctionne sur tout layout surround.
fn find_stereo_config(
    device: &cpal::Device,
    desired_rate: SampleRate,
) -> Result<StreamConfig> {
    // 1er choix : stéréo exacte F32 à 48 kHz
    for supported in device.supported_output_configs()? {
        if supported.channels() == 2
            && supported.sample_format() == SampleFormat::F32
            && supported.min_sample_rate() <= desired_rate
            && supported.max_sample_rate() >= desired_rate
        {
            return Ok(StreamConfig {
                channels: 2,
                sample_rate: desired_rate,
                buffer_size: cpal::BufferSize::Default,
            });
        }
    }

    // 2e choix : n'importe quel layout (5.1, 7.1…) F32 à 48 kHz
    // → on conserve le nombre de canaux natif pour éviter l'erreur WASAPI
    for supported in device.supported_output_configs()? {
        if supported.channels() >= 2
            && supported.sample_format() == SampleFormat::F32
            && supported.min_sample_rate() <= desired_rate
            && supported.max_sample_rate() >= desired_rate
        {
            return Ok(StreamConfig {
                channels: supported.channels(),
                sample_rate: desired_rate,
                buffer_size: cpal::BufferSize::Default,
            });
        }
    }

    // Fallback absolu : config par défaut du périphérique
    let conf = device.default_output_config()?;
    Ok(StreamConfig {
        channels: conf.channels(),
        sample_rate: conf.sample_rate(),
        buffer_size: cpal::BufferSize::Default,
    })
}

/// Cherche une config mono (ou stéréo en fallback) à 48 kHz sur le micro.
fn find_mono_input_config(
    device: &cpal::Device,
    desired_rate: SampleRate,
) -> Result<StreamConfig> {
    for supported in device.supported_input_configs()? {
        if supported.sample_format() == SampleFormat::F32
            && supported.min_sample_rate() <= desired_rate
            && supported.max_sample_rate() >= desired_rate
        {
            let channels = supported.channels().min(2);
            return Ok(StreamConfig {
                channels,
                sample_rate: desired_rate,
                buffer_size: cpal::BufferSize::Default,
            });
        }
    }

    let conf = device.default_input_config()?;
    Ok(StreamConfig {
        channels: conf.channels(),
        sample_rate: conf.sample_rate(),
        buffer_size: cpal::BufferSize::Default,
    })
}

/// Retourne le nom du périphérique d'entrée et de sortie par défaut.
pub fn default_device_names() -> (String, String) {
    let host = cpal::default_host();
    let out = host
        .default_output_device()
        .map(|d| d.name().unwrap_or_else(|_| "Inconnu".into()))
        .unwrap_or_else(|| "Aucun".into());
    let inp = host
        .default_input_device()
        .map(|d| d.name().unwrap_or_else(|_| "Inconnu".into()))
        .unwrap_or_else(|| "Aucun".into());
    (out, inp)
}
