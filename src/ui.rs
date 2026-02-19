// ============================================================
//  ui.rs — Interface TUI avec ratatui
//
//  Reproduit fidèlement l'interface de l'app React :
//    - En-tête + statut micro/sortie
//    - Sélecteur de signal (Sweep / Bruit rose)
//    - Boutons de capture gauche / droite
//    - Barre de progression pendant la capture
//    - Visualisation spectrale ASCII (graphique en ligne)
//    - Score ring (en ASCII), métriques, recommandations
//    - Historique des mesures
//    - Aide clavier en bas
// ============================================================

use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    symbols,
    text::{Line, Span},
    widgets::{
        Axis, Block, Borders, Chart, Dataset, Gauge, GraphType, List, ListItem, Paragraph, Wrap,
    },
};

use crate::{
    app::{AppState, Step},
    dsp::NUM_BANDS,
};

// ─── Palette ──────────────────────────────────────────────────────────────────

const GREEN: Color = Color::Rgb(0, 255, 135);
const ORANGE: Color = Color::Rgb(255, 107, 53);
const CYAN: Color = Color::Rgb(0, 204, 255);
const RED: Color = Color::Rgb(255, 45, 85);
const YELLOW: Color = Color::Rgb(255, 214, 10);
const PURPLE: Color = Color::Rgb(168, 85, 247);
const DARK: Color = Color::Rgb(20, 20, 35);
const GRAY: Color = Color::Rgb(80, 80, 100);
const WHITE: Color = Color::Rgb(220, 220, 230);

fn score_color(score: u32) -> Color {
    if score >= 85 { GREEN } else if score >= 60 { YELLOW } else { RED }
}

// ─── Point d'entrée du rendu ──────────────────────────────────────────────────

pub fn draw(f: &mut Frame, state: &AppState) {
    let area = f.area();

    // Layout principal vertical
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(1)
        .constraints([
            Constraint::Length(4),  // Header
            Constraint::Length(3),  // Signal selector + devices
            Constraint::Length(5),  // Capture controls
            Constraint::Length(3),  // Progress / status bar
            Constraint::Min(12),    // Spectrum + results
            Constraint::Length(3),  // Keyboard help
        ])
        .split(area);

    draw_header(f, chunks[0], state);
    draw_delay_control(f, chunks[1], state);
    draw_capture_controls(f, chunks[2], state);
    draw_progress(f, chunks[3], state);

    // Zone centrale : spectre à gauche, résultats à droite
    let center = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
        .split(chunks[4]);

    draw_spectrum(f, center[0], state);
    draw_results_panel(f, center[1], state);

    draw_help(f, chunks[5], state);
}

// ─── En-tête ──────────────────────────────────────────────────────────────────

fn draw_header(f: &mut Frame, area: Rect, state: &AppState) {
    let mic_dot = if state.step == Step::CapturingLeft
        || state.step == Step::CapturingRight
    {
        Span::styled("◉ REC", Style::default().fg(RED).add_modifier(Modifier::BOLD))
    } else {
        Span::styled("● PRÊT", Style::default().fg(GREEN))
    };

    let title = Line::from(vec![
        Span::styled(
            "  Speaker Align  ",
            Style::default()
                .fg(WHITE)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("  "),
        mic_dot,
    ]);

    let subtitle = Line::from(vec![Span::styled(
        "  Calibration de placement stéréo par analyse comparative micro",
        Style::default().fg(GRAY),
    )]);

    let device_line = Line::from(vec![
        Span::styled("  Sortie : ", Style::default().fg(GRAY)),
        Span::styled(&state.out_device, Style::default().fg(CYAN)),
        Span::styled("   Entrée : ", Style::default().fg(GRAY)),
        Span::styled(&state.in_device, Style::default().fg(CYAN)),
    ]);

    let block = Block::default()
        .borders(Borders::BOTTOM)
        .border_style(Style::default().fg(Color::Rgb(40, 40, 60)));

    let para = Paragraph::new(vec![title, subtitle, device_line])
        .block(block)
        .wrap(Wrap { trim: true });

    f.render_widget(para, area);
}

// ─── Contrôle du délai pré-capture ───────────────────────────────────────────

fn draw_delay_control(f: &mut Frame, area: Rect, state: &AppState) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(Span::styled(" ◈ SWEEP SINUS 20 Hz → 20 kHz  —  Délai pré-capture ", Style::default().fg(GRAY)))
        .border_style(Style::default().fg(Color::Rgb(35, 35, 50)));

    let content = Line::from(vec![
        Span::styled("  [-] ", Style::default().fg(CYAN).add_modifier(Modifier::BOLD)),
        Span::styled(
            format!("{:.1} s", state.pre_delay_secs),
            Style::default().fg(WHITE).add_modifier(Modifier::BOLD),
        ),
        Span::styled(" [+]  ", Style::default().fg(CYAN).add_modifier(Modifier::BOLD)),
        Span::styled(
            "laisser le temps au bruit transitoire de se dissiper avant la capture",
            Style::default().fg(GRAY),
        ),
    ]);

    f.render_widget(Paragraph::new(content).block(block), area);
}

// ─── Boutons de capture ───────────────────────────────────────────────────────

fn draw_capture_controls(f: &mut Frame, area: Rect, state: &AppState) {
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(area);

    // ── Gauche ──
    let left_done = state.left_db.is_some();
    let capturing_left = state.step == Step::CapturingLeft;
    let left_color = if capturing_left { GREEN } else if left_done { Color::Rgb(0, 120, 70) } else { GREEN };

    let left_status = if capturing_left {
        format!("  ◉ Capture en cours… {:.0}%", state.progress * 100.0)
    } else if left_done {
        "  ✓ Capturé — Appuyer sur [L] pour recapturer".to_string()
    } else {
        "  [L] Capturer l'enceinte GAUCHE (référence)".to_string()
    };

    let left_block = Block::default()
        .borders(Borders::ALL)
        .title(Span::styled(" L  ENCEINTE GAUCHE ", Style::default().fg(GREEN).add_modifier(Modifier::BOLD)))
        .border_style(Style::default().fg(if left_done { Color::Rgb(0, 100, 60) } else { Color::Rgb(0, 60, 35) }))
        .style(Style::default().bg(Color::Rgb(0, 12, 8)));

    let left_lines = vec![
        Line::from(Span::styled(left_status, Style::default().fg(left_color).add_modifier(Modifier::BOLD))),
        Line::from(Span::styled("  Signal test lu sur le canal GAUCHE uniquement", Style::default().fg(GRAY))),
    ];
    f.render_widget(Paragraph::new(left_lines).block(left_block), cols[0]);

    // ── Droite ──
    let right_done = state.right_db.is_some();
    let capturing_right = state.step == Step::CapturingRight;
    let right_color = if capturing_right { ORANGE } else if right_done { Color::Rgb(160, 70, 30) } else { ORANGE };

    let right_status = if capturing_right {
        format!("  ◉ Capture en cours… {:.0}%", state.progress * 100.0)
    } else if right_done {
        "  ✓ Capturé — Appuyer sur [R] pour recapturer".to_string()
    } else {
        "  [R] Capturer l'enceinte DROITE (à aligner)".to_string()
    };

    let right_block = Block::default()
        .borders(Borders::ALL)
        .title(Span::styled(" R  ENCEINTE DROITE ", Style::default().fg(ORANGE).add_modifier(Modifier::BOLD)))
        .border_style(Style::default().fg(if right_done { Color::Rgb(120, 55, 20) } else { Color::Rgb(70, 35, 15) }))
        .style(Style::default().bg(Color::Rgb(10, 6, 3)));

    let right_lines = vec![
        Line::from(Span::styled(right_status, Style::default().fg(right_color).add_modifier(Modifier::BOLD))),
        Line::from(Span::styled("  Signal test lu sur le canal DROIT uniquement", Style::default().fg(GRAY))),
    ];
    f.render_widget(Paragraph::new(right_lines).block(right_block), cols[1]);
}

// ─── Barre de progression / erreur ───────────────────────────────────────────

fn draw_progress(f: &mut Frame, area: Rect, state: &AppState) {
    if let Some(err) = &state.error {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(RED));
        let para = Paragraph::new(Span::styled(
            format!(" ⚠ {}", err),
            Style::default().fg(RED),
        ))
        .block(block);
        f.render_widget(para, area);
        return;
    }

    let is_capturing = matches!(state.step, Step::CapturingLeft | Step::CapturingRight);

    if is_capturing {
        let label = if state.step == Step::CapturingLeft {
            "Capture GAUCHE"
        } else {
            "Capture DROITE"
        };
        let color = if state.step == Step::CapturingLeft { GREEN } else { ORANGE };

        let gauge_label = if state.progress < 0.01 && state.pre_delay_secs > 0.0 {
            format!("Pause {:.1}s…", state.pre_delay_secs)
        } else {
            format!("{:.0}%", state.progress * 100.0)
        };

        let gauge = Gauge::default()
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(Span::styled(format!(" {} ", label), Style::default().fg(color)))
                    .border_style(Style::default().fg(color)),
            )
            .gauge_style(Style::default().fg(color).bg(Color::Rgb(10, 10, 20)))
            .ratio(state.progress as f64)
            .label(gauge_label);

        f.render_widget(gauge, area);
    } else {
        // Affiche les actions disponibles
        let ready_for_analyze = state.left_db.is_some() && state.right_db.is_some();
        let hint = if ready_for_analyze {
            Line::from(vec![
                Span::styled("  ⚡ Les deux enceintes sont capturées — ", Style::default().fg(GRAY)),
                Span::styled("[A] Analyser", Style::default().fg(CYAN).add_modifier(Modifier::BOLD)),
            ])
        } else {
            Line::from(Span::styled(
                "  Placez le micro au point d'écoute, puis capturez l'enceinte GAUCHE (L) puis DROITE (R)",
                Style::default().fg(GRAY),
            ))
        };

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Rgb(35, 35, 50)));

        f.render_widget(Paragraph::new(hint).block(block), area);
    }
}

// ─── Visualisation spectrale ──────────────────────────────────────────────────

fn draw_spectrum(f: &mut Frame, area: Rect, state: &AppState) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(Span::styled(
            " Réponse en fréquence (dB) ",
            Style::default().fg(GRAY).add_modifier(Modifier::BOLD),
        ))
        .border_style(Style::default().fg(Color::Rgb(35, 35, 55)));

    if state.left_db.is_none() && state.right_db.is_none() {
        let para = Paragraph::new(vec![
            Line::from(""),
            Line::from(Span::styled(
                "  Capturez les deux enceintes pour afficher leur réponse en fréquence",
                Style::default().fg(GRAY),
            )),
        ])
        .block(block);
        f.render_widget(para, area);
        return;
    }

    // Niveau de référence = pic global parmi gauche et droite.
    // Normaliser par ce pic garantit que la courbe la plus forte apparaît à 0 dB
    // et que la courbe plus faible descend d'autant de dB qu'elle l'est vraiment.
    let ref_db: f32 = {
        let mut m = f32::NEG_INFINITY;
        if let Some(l) = &state.left_db {
            for &v in l { if v > m { m = v; } }
        }
        if let Some(r) = &state.right_db {
            for &v in r { if v > m { m = v; } }
        }
        if m.is_infinite() || m < -80.0 { 0.0 } else { m }
    };

    // Convertit les bandes en points (x, y) normalisés par rapport au pic global
    let make_data = |bands: &[f32]| -> Vec<(f64, f64)> {
        bands
            .iter()
            .enumerate()
            .map(|(i, &db)| (i as f64, (db - ref_db).max(-80.0) as f64))
            .collect()
    };

    // Pré-alloue les données pour garantir leur durée de vie >= datasets
    let left_data: Vec<(f64, f64)> = state.left_db.as_deref()
        .map(make_data).unwrap_or_default();
    let right_data: Vec<(f64, f64)> = state.right_db.as_deref()
        .map(make_data).unwrap_or_default();
    // La diff R-L est déjà relative, on la clamp juste sur la plage affichable
    let diff_data: Vec<(f64, f64)> = state.diff_db.as_deref()
        .map(|bands| bands.iter().enumerate()
            .map(|(i, &db)| (i as f64, db.clamp(-80.0, 0.0) as f64))
            .collect())
        .unwrap_or_default();

    let mut datasets: Vec<Dataset> = Vec::new();

    if state.left_db.is_some() {
        datasets.push(
            Dataset::default()
                .name("Gauche")
                .marker(symbols::Marker::Braille)
                .graph_type(GraphType::Line)
                .style(Style::default().fg(GREEN))
                .data(&left_data),
        );
    }
    if state.right_db.is_some() {
        datasets.push(
            Dataset::default()
                .name("Droite")
                .marker(symbols::Marker::Braille)
                .graph_type(GraphType::Line)
                .style(Style::default().fg(ORANGE))
                .data(&right_data),
        );
    }
    if state.diff_db.is_some() {
        datasets.push(
            Dataset::default()
                .name("Δ Diff")
                .marker(symbols::Marker::Dot)
                .graph_type(GraphType::Line)
                .style(Style::default().fg(RED))
                .data(&diff_data),
        );
    }

    // Étiquettes de l'axe X (fréquences)
    let freq_labels: Vec<(f64, String)> = [20usize, 50, 100, 500, 1000, 5000, 10000, 20000]
        .iter()
        .map(|&f| {
            let log_min = (20f32).log10() as f64;
            let log_max = (20_000f32).log10() as f64;
            let ratio = ((f as f64).log10() - log_min) / (log_max - log_min);
            let idx = (ratio * (NUM_BANDS - 1) as f64) as f64;
            let label = if f >= 1000 {
                format!("{}k", f / 1000)
            } else {
                format!("{}", f)
            };
            (idx, label)
        })
        .collect();

    let x_labels: Vec<Span> = freq_labels
        .iter()
        .map(|(_, l)| Span::styled(l.clone(), Style::default().fg(GRAY)))
        .collect();

    let chart = Chart::new(datasets)
        .block(block)
        .x_axis(
            Axis::default()
                .title(Span::styled("Hz", Style::default().fg(GRAY)))
                .style(Style::default().fg(GRAY))
                .labels(x_labels)
                .bounds([0.0, (NUM_BANDS - 1) as f64]),
        )
        .y_axis(
            Axis::default()
                .title(Span::styled("dB", Style::default().fg(GRAY)))
                .style(Style::default().fg(GRAY))
                .labels(vec![
                    Span::styled("-80", Style::default().fg(GRAY)),
                    Span::styled("-60", Style::default().fg(GRAY)),
                    Span::styled("-40", Style::default().fg(GRAY)),
                    Span::styled("-20", Style::default().fg(GRAY)),
                    Span::styled("0", Style::default().fg(GRAY)),
                ])
                .bounds([-80.0, 0.0]),
        );

    f.render_widget(chart, area);
}

// ─── Panneau de résultats ─────────────────────────────────────────────────────

fn draw_results_panel(f: &mut Frame, area: Rect, state: &AppState) {
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(7),  // Score + métriques
            Constraint::Min(5),     // Recommandations
            Constraint::Length(6),  // Historique
        ])
        .split(area);

    draw_score_metrics(f, rows[0], state);
    draw_recommendations(f, rows[1], state);
    draw_history(f, rows[2], state);
}

fn draw_score_metrics(f: &mut Frame, area: Rect, state: &AppState) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(Span::styled(" Score & Métriques ", Style::default().fg(GRAY)))
        .border_style(Style::default().fg(Color::Rgb(35, 35, 55)));

    if let Some(score) = state.score {
        let col = score_color(score);
        let rating = if score >= 85 { "EXCELLENT" } else if score >= 60 { "AJUSTABLE" } else { "À CORRIGER" };

        let dist_line = match (state.left_dist_m, state.right_dist_m) {
            (Some(l), Some(r)) => Line::from(vec![
                Span::styled("  Distances  ", Style::default().fg(GRAY)),
                Span::styled("G ", Style::default().fg(GREEN).add_modifier(Modifier::BOLD)),
                Span::styled(format!("{:.2} m", l), Style::default().fg(GREEN)),
                Span::styled("  D ", Style::default().fg(ORANGE).add_modifier(Modifier::BOLD)),
                Span::styled(format!("{:.2} m", r), Style::default().fg(ORANGE)),
            ]),
            _ => Line::from(Span::styled(
                "  Distances  — sweep requis",
                Style::default().fg(GRAY),
            )),
        };

        let lines = vec![
            Line::from(vec![
                Span::styled(
                    format!("  {:>3}/100 ", score),
                    Style::default().fg(col).add_modifier(Modifier::BOLD),
                ),
                Span::styled(rating, Style::default().fg(col)),
            ]),
            dist_line,
            meter_line_delay("Délai", state.delay_ms, 5.0, 0.2, CYAN),
            meter_line("Niveau", state.level_diff_db, "dB", 10.0, 0.5, ORANGE),
            meter_line("Spectre", state.freq_tilt, "dB", 10.0, 1.0, PURPLE),
        ];

        f.render_widget(Paragraph::new(lines).block(block), area);
    } else {
        let para = Paragraph::new(vec![
            Line::from(""),
            Line::from(Span::styled(
                "  Lancez l'analyse [A]",
                Style::default().fg(GRAY),
            )),
        ])
        .block(block);
        f.render_widget(para, area);
    }
}

fn meter_line(label: &str, value: f32, unit: &str, max: f32, tolerance: f32, color: Color) -> Line<'static> {
    let is_good = value.abs() <= tolerance;
    let is_ok = value.abs() <= tolerance * 2.0;
    let status_color = if is_good { GREEN } else if is_ok { YELLOW } else { RED };
    let sign = if value >= 0.0 { "+" } else { "" };
    let bar_len = 12usize;
    let filled = ((value.abs() / max).min(1.0) * bar_len as f32) as usize;
    let bar: String = "█".repeat(filled) + &"░".repeat(bar_len - filled);

    Line::from(vec![
        Span::styled(
            format!("  {:<8}", label),
            Style::default().fg(GRAY),
        ),
        Span::styled(bar, Style::default().fg(color)),
        Span::styled(
            format!(" {}{:.2} {}", sign, value, unit),
            Style::default().fg(status_color).add_modifier(Modifier::BOLD),
        ),
    ])
}

fn meter_line_delay(label: &str, value: f32, max: f32, tolerance: f32, color: Color) -> Line<'static> {
    let is_good = value.abs() <= tolerance;
    let is_ok = value.abs() <= tolerance * 2.0;
    let status_color = if is_good { GREEN } else if is_ok { YELLOW } else { RED };
    let sign = if value >= 0.0 { "+" } else { "" };
    let bar_len = 12usize;
    let filled = ((value.abs() / max).min(1.0) * bar_len as f32) as usize;
    let bar: String = "█".repeat(filled) + &"░".repeat(bar_len - filled);

    Line::from(vec![
        Span::styled(
            format!("  {:<8}", label),
            Style::default().fg(GRAY),
        ),
        Span::styled(bar, Style::default().fg(color)),
        Span::styled(
            format!(" {}{:.3} ms", sign, value),
            Style::default().fg(status_color).add_modifier(Modifier::BOLD),
        ),
    ])
}

fn draw_recommendations(f: &mut Frame, area: Rect, state: &AppState) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(Span::styled(" Recommandations ", Style::default().fg(GRAY)))
        .border_style(Style::default().fg(Color::Rgb(35, 35, 55)));

    if state.score.is_none() {
        let para = Paragraph::new(Span::styled(
            "  Résultats disponibles après analyse",
            Style::default().fg(GRAY),
        ))
        .block(block);
        f.render_widget(para, area);
        return;
    }

    let mut guides: Vec<Line> = Vec::new();

    if state.delay_ms.abs() > 0.1 {
        let icon = if state.delay_ms > 0.0 { "↗" } else { "↙" };
        let action = if state.delay_ms > 0.0 {
            "Rapprocher l'enceinte droite"
        } else {
            "Éloigner l'enceinte droite"
        };
        // delay_ms * 34.3 cm/ms = distance en cm  (vitesse du son ≈ 343 m/s)
        let dist_cm = state.delay_ms.abs() * 34.3;
        let sev = if state.delay_ms.abs() > 0.5 { RED } else { YELLOW };
        guides.push(Line::from(vec![
            Span::styled(format!("  {} ", icon), Style::default().fg(sev).add_modifier(Modifier::BOLD)),
            Span::styled(action.to_string(), Style::default().fg(WHITE)),
        ]));
        let dist_label = if dist_cm < 1.0 {
            format!("    Δ distance ≈ {:.1} mm", dist_cm * 10.0)
        } else {
            format!("    Δ distance ≈ {:.1} cm", dist_cm)
        };
        guides.push(Line::from(Span::styled(
            dist_label,
            Style::default().fg(GRAY),
        )));
    }

    if state.level_diff_db.abs() > 0.5 {
        let icon = if state.level_diff_db > 0.0 { "🔉" } else { "🔊" };
        let action = if state.level_diff_db > 0.0 {
            "Son droit trop fort — éloigner ou désaxer"
        } else {
            "Son droit trop faible — rapprocher ou orienter"
        };
        let sev = if state.level_diff_db.abs() > 2.0 { RED } else { YELLOW };
        guides.push(Line::from(vec![
            Span::styled(format!("  {} ", icon), Style::default().fg(sev)),
            Span::styled(action.to_string(), Style::default().fg(WHITE)),
        ]));
        guides.push(Line::from(Span::styled(
            format!("    Δ niveau = {:.1} dB", state.level_diff_db.abs()),
            Style::default().fg(GRAY),
        )));
    }

    if state.freq_tilt.abs() > 1.0 {
        let icon = if state.freq_tilt > 0.0 { "◑" } else { "◐" };
        let action = if state.freq_tilt > 0.0 {
            "Trop d'aigus à droite — désaxer (toe-out)"
        } else {
            "Manque d'aigus à droite — orienter (toe-in)"
        };
        let sev = if state.freq_tilt.abs() > 3.0 { RED } else { YELLOW };
        guides.push(Line::from(vec![
            Span::styled(format!("  {} ", icon), Style::default().fg(sev)),
            Span::styled(action.to_string(), Style::default().fg(WHITE)),
        ]));
    }

    if guides.is_empty() {
        guides.push(Line::from(""));
        guides.push(Line::from(Span::styled(
            "  ✓ Placement optimal atteint !",
            Style::default().fg(GREEN).add_modifier(Modifier::BOLD),
        )));
        guides.push(Line::from(Span::styled(
            "  Les deux enceintes sont symétriquement alignées.",
            Style::default().fg(GRAY),
        )));
    }

    f.render_widget(Paragraph::new(guides).block(block).wrap(Wrap { trim: true }), area);
}

fn draw_history(f: &mut Frame, area: Rect, state: &AppState) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(Span::styled(" Historique ", Style::default().fg(GRAY)))
        .border_style(Style::default().fg(Color::Rgb(35, 35, 55)));

    if state.history.is_empty() {
        let para = Paragraph::new(Span::styled("  Aucune mesure", Style::default().fg(GRAY)))
            .block(block);
        f.render_widget(para, area);
        return;
    }

    let items: Vec<ListItem> = state
        .history
        .iter()
        .enumerate()
        .rev()
        .take(4)
        .map(|(i, h)| {
            let col = score_color(h.score);
            let is_last = i == state.history.len() - 1;
            let trend = if i > 0 && is_last {
                if h.score > state.history[i - 1].score { " ↗" }
                else if h.score < state.history[i - 1].score { " ↘" }
                else { " →" }
            } else { "" };

            ListItem::new(Line::from(vec![
                Span::styled(
                    format!("  {:>3}", h.score),
                    Style::default().fg(col).add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!(" pts  Δt={:.1}ms  ΔL={:.1}dB  {}{}",
                        h.delay_ms, h.level_diff_db, h.time, trend),
                    Style::default().fg(if is_last { WHITE } else { GRAY }),
                ),
            ]))
        })
        .collect();

    let list = List::new(items).block(block);
    f.render_widget(list, area);
}

// ─── Aide clavier ─────────────────────────────────────────────────────────────

fn draw_help(f: &mut Frame, area: Rect, state: &AppState) {
    let items: Vec<(&str, &str)> = vec![
        ("[L]", "Capturer gauche"),
        ("[R]", "Capturer droite"),
        ("[A]", "Analyser"),
        ("[+/-]", "Délai pré-capture"),
        ("[X]", "Réinitialiser"),
        ("[Q]", "Quitter"),
    ];

    let spans: Vec<Span> = items
        .iter()
        .flat_map(|(key, desc)| {
            vec![
                Span::styled(format!(" {} ", key), Style::default().fg(CYAN).add_modifier(Modifier::BOLD)),
                Span::styled(format!("{} ", desc), Style::default().fg(GRAY)),
                Span::styled(" │ ", Style::default().fg(Color::Rgb(40, 40, 55))),
            ]
        })
        .collect();

    let line = Line::from(spans);
    let block = Block::default()
        .borders(Borders::TOP)
        .border_style(Style::default().fg(Color::Rgb(35, 35, 50)));

    f.render_widget(Paragraph::new(line).block(block), area);
}
