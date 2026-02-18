# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Commands

```bash
# Build (debug — opt-level 1 for audio performance)
cargo build

# Build optimized release (LTO + codegen-units=1)
cargo build --release

# Run
cargo run
./target/release/speaker-align

# Check for compile errors without producing artifacts
cargo check

# Lint
cargo clippy

# Format
cargo fmt
```

There are no automated tests in this project.

## Architecture

**speaker-align** is a native Rust TUI application for stereo speaker placement calibration. It plays a test signal through one speaker at a time, records it via microphone, then computes acoustic metrics and placement recommendations.

### Module roles

| Module | Role |
|--------|------|
| `main.rs` | Entry point — calls `App::run()` |
| `app.rs` | State machine + event loop. Owns `AppState` and drives the `Step` enum through `Idle → CapturingLeft/Right → Analyzing → Results`. Audio capture runs in a spawned thread; results are communicated back via `mpsc::channel::<AudioMsg>`. |
| `dsp.rs` | All signal processing: logarithmic sweep and pink noise generation, FFT (Hann-windowed, averaged over segments), spectrum→128 log bands, RMS, cross-correlation delay estimation, frequency tilt, and the 0–100 composite score. Exports shared constants (`SAMPLE_RATE`, `FFT_SIZE`, `NUM_BANDS`, `SWEEP_DURATION`, `CAPTURE_DURATION`). |
| `audio.rs` | cpal I/O: simultaneously plays the test signal on a single stereo channel and captures the microphone (mixed to mono f32). Negotiates 48 kHz / F32 format with fallbacks for both input and output. Progress is reported via a second `mpsc` channel. |
| `ui.rs` | ratatui rendering. Single `draw()` entry point that composes a fixed vertical layout: header → signal selector → capture controls → progress bar → [spectrum chart | results panel] → key-bindings help. The spectrum uses Braille markers. |

### Data flow

1. User presses `L` or `R` → `AppState::start_capture()` spawns a thread that calls `audio::play_and_capture()`.
2. Thread sends `AudioMsg::Progress(f32)` periodically and `AudioMsg::Done(Vec<f32>)` on completion.
3. Main loop's `poll_audio()` receives messages; on `Done`, `run_dsp()` runs FFT → bands → dB and stores results in `AppState`.
4. User presses `A` → `AppState::analyze()` computes delay (cross-correlation), level diff (RMS ratio), spectral diff, freq tilt, and composite score synchronously (no thread). Appends a `HistoryEntry`.
5. `ui::draw()` reads `AppState` immutably every 50 ms tick.

### Key constants (all in `dsp.rs`)

- `SAMPLE_RATE` = 48 000 Hz
- `FFT_SIZE` = 8 192 points
- `NUM_BANDS` = 128 logarithmic bands (20 Hz – 20 kHz)
- `SWEEP_DURATION` = 3 s, `CAPTURE_DURATION` = 4 s

### Score breakdown

- Spectral similarity: 0–50 pts (mean absolute dB diff across bands)
- Level balance: 0–25 pts (RMS diff in dB)
- Timing alignment: 0–25 pts (cross-correlation delay in ms)
- Score ≥ 85 = optimal placement
