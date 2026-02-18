// ============================================================
//  Speaker Align — Calibration acoustique stéréo
//  Réécriture native Rust de l'app React/Web Audio API
//
//  Dépendances :
//    cpal     — capture & lecture audio cross-platform
//    rustfft  — FFT rapide O(n log n)
//    ratatui  — interface TUI
//    crossterm — terminal cross-platform
// ============================================================

mod audio;
mod dsp;
mod ui;
mod app;

use anyhow::Result;
use app::App;

fn main() -> Result<()> {
    App::run()
}
