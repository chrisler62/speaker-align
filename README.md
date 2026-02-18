# Speaker Align 🔊

Calibration de placement stéréo par analyse comparative micro — application native Rust.

Réécriture complète de l'app React/Web Audio API en application terminal (TUI).

## Fonctionnalités

- **Sweep sinusoïdal logarithmique** 20Hz–20kHz ou **bruit rose** (Voss-McCartney)
- **FFT rapide O(n log n)** via `rustfft` avec fenêtre de Hann
- **Découpage en 128 bandes logarithmiques** (20Hz–20kHz)
- **Corrélation croisée** pour estimer le délai inter-canal (en ms → en cm)
- **Différence de niveau** RMS gauche/droite (en dB)
- **Inclinaison spectrale** (tilt hautes/basses fréquences)
- **Score global 0–100** (fréquence + niveau + temps)
- **Recommandations de placement** (rapprocher, éloigner, toe-in, toe-out)
- **Historique** des mesures avec tendance
- **Visualisation spectrale** en temps réel (graphique Braille dans le terminal)

## Prérequis

- Rust 1.75+ (`rustup`)
- Un microphone branché au point d'écoute
- Des enceintes stéréo actives

## Installation

```bash
git clone <repo>
cd speaker-align
cargo build --release
./target/release/speaker-align
```

## Utilisation

```
[L]   Capturer l'enceinte gauche (signal joué uniquement à gauche)
[R]   Capturer l'enceinte droite (signal joué uniquement à droite)
[A]   Analyser et comparer les deux captures
[Tab] Basculer entre Sweep sinus et Bruit rose
[X]   Réinitialiser les mesures
[Q]   Quitter
```

## Procédure

1. Placez le microphone au **point d'écoute** (position de l'auditeur)
2. Appuyez sur **[L]** — le sweep est joué à gauche, le micro enregistre
3. Appuyez sur **[R]** — le sweep est joué à droite, le micro enregistre
4. Appuyez sur **[A]** pour lancer l'analyse comparative
5. Lisez les recommandations et ajustez l'enceinte droite
6. Répétez jusqu'à obtenir un **score ≥ 85** (placement optimal)

## Architecture

```
src/
├── main.rs      Point d'entrée
├── dsp.rs       Traitement du signal (FFT, bandes, RMS, délai, score)
├── audio.rs     Lecture & capture audio via cpal
├── app.rs       Machine d'état (Step: Idle → Capturing → Analyzing → Results)
└── ui.rs        Interface TUI via ratatui (spectre, score, métriques, historique)
```

## Dépendances

| Crate      | Rôle                              |
|------------|-----------------------------------|
| `cpal`     | Audio I/O cross-platform          |
| `rustfft`  | FFT O(n log n)                   |
| `ratatui`  | TUI (terminal user interface)     |
| `crossterm`| Terminal cross-platform           |
| `anyhow`   | Gestion d'erreurs ergonomique     |
| `rand`     | Génération de bruit blanc         |

## Paramètres audio

| Paramètre       | Valeur  |
|-----------------|---------|
| Taux d'échantillonnage | 48 000 Hz |
| Taille FFT      | 8 192 points |
| Bandes          | 128 (log) |
| Durée sweep     | 3 s     |
| Durée capture   | 4 s     |
