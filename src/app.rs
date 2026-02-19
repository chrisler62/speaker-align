// ============================================================
//  app.rs — Machine d'état de l'application
//
//  Gère le cycle de vie complet :
//    Idle → Capturing → Analyzing → Results → Idle…
// ============================================================

use anyhow::Result;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::{
    io,
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

use crate::{
    audio::{self, Channel},
    dsp::{self, *},
    ui,
};

// ─── Types ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Step {
    Idle,
    CapturingLeft,
    CapturingRight,
    Analyzing,
    Results,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SignalType {
    Sweep,
    PinkNoise,
}

#[derive(Debug, Clone)]
pub struct HistoryEntry {
    pub score: u32,
    pub delay_ms: f32,
    pub level_diff_db: f32,
    pub time: String,
}

// Message envoyé par les threads audio vers la boucle principale
pub enum AudioMsg {
    Progress(f32),
    Done(Vec<f32>),
    Error(String),
}

pub struct AppState {
    pub step: Step,
    pub signal_type: SignalType,

    // Données brutes
    pub left_samples: Option<Vec<f32>>,
    pub right_samples: Option<Vec<f32>>,

    // Résultats DSP
    pub left_db: Option<Vec<f32>>,
    pub right_db: Option<Vec<f32>>,
    pub diff_db: Option<Vec<f32>>,

    pub delay_ms: f32,
    pub level_diff_db: f32,
    pub freq_tilt: f32,
    pub score: Option<u32>,
    pub progress: f32,

    pub error: Option<String>,
    pub history: Vec<HistoryEntry>,

    pub out_device: String,
    pub in_device: String,

    // Délai pré-capture (secondes) — évite d'enregistrer la frappe clavier
    pub pre_delay_secs: f32,

    // Canal de communication inter-thread
    pub audio_rx: Option<mpsc::Receiver<AudioMsg>>,
}

impl AppState {
    pub fn new() -> Self {
        let (out, inp) = audio::default_device_names();
        AppState {
            step: Step::Idle,
            signal_type: SignalType::PinkNoise,
            left_samples: None,
            right_samples: None,
            left_db: None,
            right_db: None,
            diff_db: None,
            delay_ms: 0.0,
            level_diff_db: 0.0,
            freq_tilt: 0.0,
            score: None,
            progress: 0.0,
            error: None,
            history: Vec::new(),
            out_device: out,
            in_device: inp,
            pre_delay_secs: 1.0,
            audio_rx: None,
        }
    }

    /// Lance la capture pour le canal donné dans un thread séparé.
    pub fn start_capture(&mut self, channel: Channel) {
        let (tx, rx) = mpsc::channel::<AudioMsg>();
        self.audio_rx = Some(rx);
        self.progress = 0.0;
        self.error = None;

        let signal_type = self.signal_type;
        let pre_delay_secs = self.pre_delay_secs;

        thread::spawn(move || {
            // Génère le signal de test
            let signal = match signal_type {
                SignalType::Sweep => dsp::generate_sweep(SAMPLE_RATE, SWEEP_DURATION),
                SignalType::PinkNoise => dsp::generate_pink_noise(SAMPLE_RATE, SWEEP_DURATION),
            };

            let (prog_tx, prog_rx) = mpsc::channel::<f32>();

            // Thread de progression
            let tx2 = tx.clone();
            thread::spawn(move || {
                while let Ok(p) = prog_rx.recv() {
                    let _ = tx2.send(AudioMsg::Progress(p));
                }
            });

            match audio::play_and_capture(&signal, channel, CAPTURE_DURATION, pre_delay_secs, prog_tx) {
                Ok(samples) => {
                    let _ = tx.send(AudioMsg::Done(samples));
                }
                Err(e) => {
                    let _ = tx.send(AudioMsg::Error(e.to_string()));
                }
            }
        });

        self.step = match channel {
            Channel::Left => Step::CapturingLeft,
            Channel::Right => Step::CapturingRight,
        };
    }

    /// Dépile les messages audio reçus du thread de capture.
    pub fn poll_audio(&mut self) {
        let msg = if let Some(rx) = &self.audio_rx {
            rx.try_recv().ok()
        } else {
            None
        };

        match msg {
            Some(AudioMsg::Progress(p)) => self.progress = p,
            Some(AudioMsg::Done(samples)) => {
                self.run_dsp(samples);
            }
            Some(AudioMsg::Error(e)) => {
                self.error = Some(e);
                self.step = Step::Idle;
                self.audio_rx = None;
            }
            None => {}
        }
    }

    /// Calcule le spectre après réception des échantillons.
    fn run_dsp(&mut self, samples: Vec<f32>) {
        // Filtre passe-haut 30 Hz : supprime le bruit de ronflement ambiant
        // (ventilateurs PC, vibrations bureau) sans affecter la plage utile
        let filtered = dsp::highpass_filter(&samples, 30.0, SAMPLE_RATE);
        let spectrum = dsp::compute_fft(&filtered);
        let bands = dsp::spectrum_to_bands(&spectrum, SAMPLE_RATE, NUM_BANDS);
        let bands_db = dsp::bands_to_db(&bands);

        match self.step {
            Step::CapturingLeft => {
                self.left_samples = Some(filtered);
                self.left_db = Some(bands_db);
                self.step = Step::Idle;
            }
            Step::CapturingRight => {
                self.right_samples = Some(filtered);
                self.right_db = Some(bands_db);
                self.step = Step::Idle;
            }
            _ => {}
        }
        self.audio_rx = None;
    }

    /// Lance l'analyse comparative une fois les deux captures effectuées.
    pub fn analyze(&mut self) {
        let (left_s, right_s) = match (&self.left_samples, &self.right_samples) {
            (Some(l), Some(r)) => (l.clone(), r.clone()),
            _ => return,
        };

        let (left_db, right_db) = match (&self.left_db, &self.right_db) {
            (Some(l), Some(r)) => (l.clone(), r.clone()),
            _ => return,
        };

        self.step = Step::Analyzing;

        // Délai inter-canal
        let delay = dsp::compute_delay(&left_s, &right_s, SAMPLE_RATE);
        self.delay_ms = delay * 1000.0;

        // Différence de niveau (RMS)
        let left_rms = dsp::compute_rms(&left_s);
        let right_rms = dsp::compute_rms(&right_s);
        self.level_diff_db = if left_rms > 0.0 && right_rms > 0.0 {
            20.0 * (right_rms / left_rms).log10()
        } else {
            0.0
        };

        // Différence spectrale
        let diff: Vec<f32> = left_db
            .iter()
            .zip(right_db.iter())
            .map(|(l, r)| r - l)
            .collect();
        self.diff_db = Some(diff);

        // Inclinaison spectrale
        self.freq_tilt = dsp::compute_freq_tilt(&left_db, &right_db);

        // Score global
        let s = dsp::compute_score(&left_db, &right_db, self.delay_ms, self.level_diff_db);
        self.score = Some(s);

        // Historique
        let now = chrono_now();
        self.history.push(HistoryEntry {
            score: s,
            delay_ms: self.delay_ms,
            level_diff_db: self.level_diff_db,
            time: now,
        });

        self.step = Step::Results;
    }

    /// Réinitialise les mesures (garde l'historique).
    pub fn reset(&mut self) {
        self.left_samples = None;
        self.right_samples = None;
        self.left_db = None;
        self.right_db = None;
        self.diff_db = None;
        self.delay_ms = 0.0;
        self.level_diff_db = 0.0;
        self.freq_tilt = 0.0;
        self.score = None;
        self.progress = 0.0;
        self.error = None;
        self.step = Step::Idle;
    }
}

fn chrono_now() -> String {
    // Heure système simplifiée (sans dépendance chrono)
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let h = (secs % 86400) / 3600;
    let m = (secs % 3600) / 60;
    let s = secs % 60;
    format!("{:02}:{:02}:{:02}", h, m, s)
}

// ─── Point d'entrée ───────────────────────────────────────────────────────────

pub struct App;

impl App {
    pub fn run() -> Result<()> {
        // Init terminal
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend)?;

        let mut state = AppState::new();
        let tick = Duration::from_millis(50);
        let mut last_tick = Instant::now();

        loop {
            // Dépile les messages audio
            state.poll_audio();

            // Rendu
            terminal.draw(|f| ui::draw(f, &state))?;

            // Gestion des événements clavier
            let timeout = tick.checked_sub(last_tick.elapsed()).unwrap_or_default();
            if event::poll(timeout)? {
                if let Event::Key(key) = event::read()? {
                    if key.kind != KeyEventKind::Press {
                        continue;
                    }
                    match (key.code, key.modifiers) {
                        // Quitter
                        (KeyCode::Char('q'), _)
                        | (KeyCode::Char('c'), KeyModifiers::CONTROL) => break,

                        // Capturer gauche
                        (KeyCode::Char('l') | KeyCode::Char('L'), _)
                            if state.step == Step::Idle =>
                        {
                            state.start_capture(Channel::Left);
                        }

                        // Capturer droite
                        (KeyCode::Char('r') | KeyCode::Char('R'), _)
                            if state.step == Step::Idle =>
                        {
                            state.start_capture(Channel::Right);
                        }

                        // Analyser
                        (KeyCode::Char('a') | KeyCode::Enter, _)
                            if state.step == Step::Idle
                                && state.left_db.is_some()
                                && state.right_db.is_some() =>
                        {
                            state.analyze();
                        }

                        // Réinitialiser
                        (KeyCode::Char('x') | KeyCode::Delete, _) => {
                            state.reset();
                        }

                        // Changer le type de signal
                        (KeyCode::Tab, _) if state.step == Step::Idle => {
                            state.signal_type = match state.signal_type {
                                SignalType::Sweep => SignalType::PinkNoise,
                                SignalType::PinkNoise => SignalType::Sweep,
                            };
                        }

                        // Augmenter le délai pré-capture (+0.5s, max 5.0s)
                        (KeyCode::Char('+') | KeyCode::Char('='), _)
                            if state.step == Step::Idle =>
                        {
                            state.pre_delay_secs = (state.pre_delay_secs + 0.5).min(5.0);
                        }

                        // Diminuer le délai pré-capture (-0.5s, min 0.0s)
                        (KeyCode::Char('-'), _) if state.step == Step::Idle => {
                            state.pre_delay_secs = (state.pre_delay_secs - 0.5).max(0.0);
                        }

                        _ => {}
                    }
                }
            }

            if last_tick.elapsed() >= tick {
                last_tick = Instant::now();
            }
        }

        // Restaure le terminal
        disable_raw_mode()?;
        execute!(
            terminal.backend_mut(),
            LeaveAlternateScreen,
            DisableMouseCapture
        )?;
        terminal.show_cursor()?;
        Ok(())
    }
}
