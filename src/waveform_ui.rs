use crate::document::AudioDocument;
use crate::playback::MeterReading;
use crate::theme::{amp_to_db, format_timecode, ForgeTheme};
use egui::{Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, Ui, Vec2};

pub struct WaveformInteraction {
    pub drag_anchor_frame: Option<usize>,
    pub overview_dragging: bool,
}

impl Default for WaveformInteraction {
    fn default() -> Self {
        Self {
            drag_anchor_frame: None,
            overview_dragging: false,
        }
    }
}

/// Renders the Sound Forge-style overview strip + time ruler + waveform + dB scale.
/// Returns `Some(frame)` if the user double-clicked or requested immediate play-from-frame.
pub fn draw_waveform_workspace(
    ui: &mut Ui,
    doc: &mut AudioDocument,
    interaction: &mut WaveformInteraction,
    playhead_frame: Option<usize>,
    snap_zero_crossing: bool,
) -> Option<usize> {
    let mut play_request = None;

    // 1. Overview Minimap Strip (height 38px)
    draw_overview_bar(ui, doc, interaction);
    ui.add_space(2.0);

    // 2. Main Waveform + Rulers
    let avail = ui.available_size();
    let total_rect = ui.allocate_exact_size(
        Vec2::new(avail.x.max(200.0), avail.y.max(180.0)),
        Sense::click_and_drag(),
    );
    let (rect, response) = total_rect;

    let ruler_h = 24.0;
    let db_ruler_w = 40.0;
    let ruler_rect = Rect::from_min_max(
        rect.min,
        Pos2::new((rect.max.x - db_ruler_w).max(rect.min.x + 50.0), rect.min.y + ruler_h),
    );
    let wave_rect = Rect::from_min_max(
        Pos2::new(rect.min.x, rect.min.y + ruler_h),
        Pos2::new((rect.max.x - db_ruler_w).max(rect.min.x + 50.0), rect.max.y),
    );
    let db_rect = Rect::from_min_max(
        Pos2::new(wave_rect.max.x, wave_rect.min.y),
        rect.max,
    );

    let painter = ui.painter_at(rect);
    painter.rect_filled(ruler_rect, 0.0, ForgeTheme::RULER_BG);
    painter.rect_filled(wave_rect, 0.0, ForgeTheme::CANVAS_BG);
    painter.rect_filled(db_rect, 0.0, ForgeTheme::RULER_BG);
    painter.rect_stroke(rect, 0.0, Stroke::new(1.0, ForgeTheme::BORDER));

    let total_frames = doc.buffer.num_frames();
    if total_frames == 0 {
        painter.text(
            wave_rect.center(),
            Align2::CENTER_CENTER,
            "Sem áudio carregado",
            FontId::proportional(14.0),
            ForgeTheme::TEXT_MUTED,
        );
        return None;
    }

    let sr = doc.buffer.sample_rate as f64;
    let view_start = doc.view_start;
    let view_frames = doc.view_frames.max(1.0);

    let x_to_frame = |x: f32| -> usize {
        let ratio = ((x - wave_rect.min.x) / wave_rect.width().max(1.0)).clamp(0.0, 1.0) as f64;
        let f = (view_start + ratio * view_frames).round() as usize;
        f.min(total_frames)
    };

    // Handle mouse wheel zoom & pan when hovering wave_rect
    if response.hovered() {
        let scroll = ui.input(|i| i.smooth_scroll_delta);
        let shift = ui.input(|i| i.modifiers.shift);
        let ctrl = ui.input(|i| i.modifiers.ctrl);

        if scroll.x.abs() > 0.1 || (shift && scroll.y.abs() > 0.1) {
            let delta = if scroll.x.abs() > 0.1 {
                -scroll.x
            } else {
                -scroll.y
            };
            let pan_frames = (delta as f64 / wave_rect.width().max(1.0) as f64) * view_frames * 0.5;
            doc.view_start += pan_frames;
            doc.clamp_viewport();
        } else if scroll.y.abs() > 0.1 {
            let zoom_factor = if scroll.y > 0.0 { 0.80 } else { 1.25 };
            let anchor = response
                .hover_pos()
                .map(|p| ((p.x - wave_rect.min.x) / wave_rect.width().max(1.0)) as f64)
                .unwrap_or(0.5);
            doc.zoom_at(zoom_factor, anchor);
            let _ = ctrl;
        }
    }

    // Handle pointer interactions (selection, cursor, pan with middle button)
    let middle_down = ui.input(|i| i.pointer.middle_down());
    if response.dragged_by(egui::PointerButton::Middle) || (middle_down && response.dragged()) {
        let dx = response.drag_delta().x;
        let pan_frames = -(dx as f64 / wave_rect.width().max(1.0) as f64) * view_frames;
        doc.view_start += pan_frames;
        doc.clamp_viewport();
    } else if response.drag_started_by(egui::PointerButton::Primary) {
        if let Some(pos) = response.interact_pointer_pos() {
            if wave_rect.contains(pos) || ruler_rect.contains(pos) {
                let mut f = x_to_frame(pos.x);
                if snap_zero_crossing {
                    let search = (doc.buffer.sample_rate as usize / 80).max(32);
                    f = doc.buffer.find_nearest_zero_crossing(f, search);
                }
                interaction.drag_anchor_frame = Some(f);
                doc.cursor_frame = f;
                doc.selection = None;
            }
        }
    } else if response.dragged_by(egui::PointerButton::Primary) {
        if let (Some(anchor), Some(pos)) = (
            interaction.drag_anchor_frame,
            response.interact_pointer_pos(),
        ) {
            let mut f = x_to_frame(pos.x);
            if snap_zero_crossing {
                let search = (doc.buffer.sample_rate as usize / 80).max(32);
                f = doc.buffer.find_nearest_zero_crossing(f, search);
            }
            if f != anchor {
                let (s, e) = if anchor < f { (anchor, f) } else { (f, anchor) };
                doc.selection = Some((s, e));
                doc.cursor_frame = s;
            } else {
                doc.selection = None;
            }
        }
    } else if response.drag_stopped() {
        interaction.drag_anchor_frame = None;
    } else if response.clicked_by(egui::PointerButton::Primary) {
        if let Some(pos) = response.interact_pointer_pos() {
            if wave_rect.contains(pos) || ruler_rect.contains(pos) {
                let mut f = x_to_frame(pos.x);
                if snap_zero_crossing {
                    let search = (doc.buffer.sample_rate as usize / 80).max(32);
                    f = doc.buffer.find_nearest_zero_crossing(f, search);
                }
                let shift = ui.input(|i| i.modifiers.shift);
                if shift {
                    let anchor = doc.cursor_frame;
                    if f != anchor {
                        let (s, e) = if anchor < f { (anchor, f) } else { (f, anchor) };
                        doc.selection = Some((s, e));
                    }
                } else {
                    doc.cursor_frame = f;
                    doc.selection = None;
                }
            }
        }
    }

    if response.double_clicked() {
        if let Some(pos) = response.interact_pointer_pos() {
            if wave_rect.contains(pos) {
                let click_f = x_to_frame(pos.x);
                // If markers bracket the click, select between surrounding markers (Sound Forge style!)
                let prev_m = doc
                    .annotations
                    .markers
                    .iter()
                    .rev()
                    .find(|m| m.frame <= click_f)
                    .map(|m| m.frame);
                let next_m = doc
                    .annotations
                    .markers
                    .iter()
                    .find(|m| m.frame > click_f)
                    .map(|m| m.frame);
                match (prev_m, next_m) {
                    (Some(a), Some(b)) if b > a => doc.selection = Some((a, b)),
                    _ => doc.selection = Some((0, total_frames)),
                }
            }
        }
    }

    // Re-read view bounds in case zoom/pan changed this frame
    let view_start = doc.view_start;
    let view_frames = doc.view_frames.max(1.0);
    let view_end = view_start + view_frames;
    let frame_to_x = |f: f64| -> f32 {
        let ratio = ((f - view_start) / view_frames) as f32;
        wave_rect.min.x + ratio * wave_rect.width()
    };

    // Draw Time Ruler ticks
    draw_time_ruler(&painter, ruler_rect, view_start, view_end, sr, &frame_to_x);

    // Draw channels
    let channels = doc.buffer.channels.max(1);
    let chan_h = wave_rect.height() / channels as f32;
    let wave_painter = painter.with_clip_rect(wave_rect);

    for ch in 0..channels {
        let ch_rect = Rect::from_min_max(
            Pos2::new(wave_rect.min.x, wave_rect.min.y + ch as f32 * chan_h),
            Pos2::new(wave_rect.max.x, wave_rect.min.y + (ch + 1) as f32 * chan_h),
        );
        if ch % 2 == 1 {
            wave_painter.rect_filled(ch_rect, 0.0, ForgeTheme::CANVAS_ALT_BG);
        }

        let mid_y = ch_rect.center().y;
        let half_h = (ch_rect.height() * 0.46).max(4.0);

        // Horizontal dB grid lines (-6 dB ~ 0.501, -12 dB ~ 0.251)
        for &amp in &[0.5_f32, 0.25_f32] {
            let dy = amp * half_h;
            wave_painter.line_segment(
                [Pos2::new(ch_rect.min.x, mid_y - dy), Pos2::new(ch_rect.max.x, mid_y - dy)],
                Stroke::new(1.0, ForgeTheme::GRID_MAJOR),
            );
            wave_painter.line_segment(
                [Pos2::new(ch_rect.min.x, mid_y + dy), Pos2::new(ch_rect.max.x, mid_y + dy)],
                Stroke::new(1.0, ForgeTheme::GRID_MAJOR),
            );
        }
        // Zero center line
        wave_painter.line_segment(
            [Pos2::new(ch_rect.min.x, mid_y), Pos2::new(ch_rect.max.x, mid_y)],
            Stroke::new(1.0, ForgeTheme::GRID_ZERO),
        );

        // Channel separator
        if ch + 1 < channels {
            wave_painter.line_segment(
                [
                    Pos2::new(ch_rect.min.x, ch_rect.max.y),
                    Pos2::new(ch_rect.max.x, ch_rect.max.y),
                ],
                Stroke::new(1.0, ForgeTheme::BORDER),
            );
        }

        // Draw waveform samples or peaks per pixel column
        let px_width = wave_rect.width().ceil().max(1.0) as usize;
        let frames_per_px = view_frames / px_width as f64;

        if frames_per_px <= 1.2 {
            // Sample-level polyline when deeply zoomed in
            let s_f = (view_start.floor() as usize).saturating_sub(1);
            let e_f = ((view_end.ceil() as usize) + 2).min(total_frames);
            let mut pts = Vec::with_capacity(e_f.saturating_sub(s_f));
            for f in s_f..e_f {
                let x = frame_to_x(f as f64);
                let s = doc.buffer.get_sample(f, ch).clamp(-1.0, 1.0);
                let y = mid_y - s * half_h;
                pts.push(Pos2::new(x, y));
            }
            if pts.len() >= 2 {
                wave_painter.add(egui::Shape::line(
                    pts,
                    Stroke::new(1.5, ForgeTheme::WAVE_OUTLINE),
                ));
            }
        } else {
            let mut prev_top: Option<Pos2> = None;
            let mut prev_bot: Option<Pos2> = None;
            for px in 0..px_width {
                let f0 = ((view_start + px as f64 * frames_per_px).floor() as usize)
                    .min(total_frames);
                let f1 = ((view_start + (px + 1) as f64 * frames_per_px).ceil() as usize)
                    .clamp(f0 + 1, total_frames);
                if f0 >= total_frames {
                    break;
                }
                let (min_v, max_v) = doc.pyramid.query_range(&doc.buffer, ch, f0, f1);
                let x = wave_rect.min.x + px as f32 + 0.5;
                let y_top = mid_y - max_v.clamp(-1.0, 1.0) * half_h;
                let y_bot = mid_y - min_v.clamp(-1.0, 1.0) * half_h;

                wave_painter.line_segment(
                    [Pos2::new(x, y_top), Pos2::new(x, y_bot.max(y_top + 1.0))],
                    Stroke::new(1.0, ForgeTheme::WAVE_FILL),
                );
                let p_top = Pos2::new(x, y_top);
                let p_bot = Pos2::new(x, y_bot);
                if let Some(pt) = prev_top {
                    wave_painter.line_segment([pt, p_top], Stroke::new(1.0, ForgeTheme::WAVE_OUTLINE));
                }
                if let Some(pb) = prev_bot {
                    wave_painter.line_segment([pb, p_bot], Stroke::new(1.0, ForgeTheme::WAVE_OUTLINE));
                }
                prev_top = Some(p_top);
                prev_bot = Some(p_bot);
            }
        }

        // Channel badge ("L" / "R" / "M")
        let label = if channels == 1 {
            "M"
        } else if ch == 0 {
            "L"
        } else {
            "R"
        };
        wave_painter.text(
            Pos2::new(ch_rect.min.x + 6.0, ch_rect.min.y + 4.0),
            Align2::LEFT_TOP,
            label,
            FontId::monospace(11.0),
            ForgeTheme::TEXT_MUTED,
        );

        // Draw dB ruler labels for this channel
        let db_ch_rect = Rect::from_min_max(
            Pos2::new(db_rect.min.x, ch_rect.min.y),
            Pos2::new(db_rect.max.x, ch_rect.max.y),
        );
        draw_db_ruler(&painter, db_ch_rect, mid_y, half_h);
    }

    // Draw Regions
    for reg in &doc.annotations.regions {
        let x0 = frame_to_x(reg.start_frame as f64);
        let x1 = frame_to_x(reg.end_frame as f64);
        if x1 >= wave_rect.min.x && x0 <= wave_rect.max.x {
            let r_rect = Rect::from_min_max(
                Pos2::new(x0.max(wave_rect.min.x), wave_rect.min.y),
                Pos2::new(x1.min(wave_rect.max.x), wave_rect.max.y),
            );
            wave_painter.rect_filled(r_rect, 0.0, ForgeTheme::REGION_FILL);
            wave_painter.line_segment(
                [Pos2::new(x0, wave_rect.min.y), Pos2::new(x0, wave_rect.max.y)],
                Stroke::new(1.0, ForgeTheme::REGION_BORDER),
            );
            wave_painter.line_segment(
                [Pos2::new(x1, wave_rect.min.y), Pos2::new(x1, wave_rect.max.y)],
                Stroke::new(1.0, ForgeTheme::REGION_BORDER),
            );
            wave_painter.text(
                Pos2::new(r_rect.min.x + 4.0, wave_rect.min.y + 4.0),
                Align2::LEFT_TOP,
                &reg.name,
                FontId::proportional(11.0),
                ForgeTheme::REGION_BORDER,
            );
        }
    }

    // Draw Selection overlay
    if let Some((s, e)) = doc.selection {
        let x0 = frame_to_x(s as f64);
        let x1 = frame_to_x(e as f64);
        if x1 >= wave_rect.min.x && x0 <= wave_rect.max.x {
            let sel_rect = Rect::from_min_max(
                Pos2::new(x0.max(wave_rect.min.x), wave_rect.min.y),
                Pos2::new(x1.min(wave_rect.max.x), wave_rect.max.y),
            );
            wave_painter.rect_filled(sel_rect, 0.0, ForgeTheme::SELECTION_FILL);
            wave_painter.line_segment(
                [Pos2::new(x0, wave_rect.min.y), Pos2::new(x0, wave_rect.max.y)],
                Stroke::new(1.5, ForgeTheme::SELECTION_BORDER),
            );
            wave_painter.line_segment(
                [Pos2::new(x1, wave_rect.min.y), Pos2::new(x1, wave_rect.max.y)],
                Stroke::new(1.5, ForgeTheme::SELECTION_BORDER),
            );
        }
    }

    // Draw Markers
    let ruler_painter = painter.with_clip_rect(ruler_rect);
    for m in &doc.annotations.markers {
        let x = frame_to_x(m.frame as f64);
        if x >= wave_rect.min.x - 10.0 && x <= wave_rect.max.x + 10.0 {
            wave_painter.line_segment(
                [Pos2::new(x, wave_rect.min.y), Pos2::new(x, wave_rect.max.y)],
                Stroke::new(1.2, ForgeTheme::MARKER),
            );
            let flag = Rect::from_min_size(
                Pos2::new(x, ruler_rect.min.y + 1.0),
                Vec2::new(56.0, 14.0),
            );
            ruler_painter.rect_filled(flag, 2.0, ForgeTheme::MARKER);
            ruler_painter.text(
                Pos2::new(x + 3.0, ruler_rect.min.y + 2.0),
                Align2::LEFT_TOP,
                &m.name,
                FontId::proportional(10.0),
                Color32::BLACK,
            );
        }
    }

    // Draw Edit Cursor
    let cur_x = frame_to_x(doc.cursor_frame as f64);
    if cur_x >= wave_rect.min.x && cur_x <= wave_rect.max.x {
        wave_painter.line_segment(
            [Pos2::new(cur_x, wave_rect.min.y), Pos2::new(cur_x, wave_rect.max.y)],
            Stroke::new(1.2, ForgeTheme::CURSOR),
        );
    }

    // Draw Live Playhead
    if let Some(pf) = playhead_frame {
        let px = frame_to_x(pf as f64);
        if px >= wave_rect.min.x && px <= wave_rect.max.x {
            wave_painter.line_segment(
                [Pos2::new(px, wave_rect.min.y), Pos2::new(px, wave_rect.max.y)],
                Stroke::new(2.0, ForgeTheme::PLAYHEAD),
            );
        }
    }

    // Context menu on right click
    response.context_menu(|ui| {
        if ui.button("Tocar daqui").clicked() {
            play_request = Some(doc.cursor_frame);
            ui.close_menu();
        }
        if ui.button("Inserir Marcador aqui (M)").clicked() {
            doc.annotations.add_marker(doc.cursor_frame, None);
            ui.close_menu();
        }
        if let Some((s, e)) = doc.selection {
            if ui.button("Criar Região da Seleção (R)").clicked() {
                doc.annotations.add_region(s, e, None);
                ui.close_menu();
            }
            if ui.button("Zoom na Seleção").clicked() {
                doc.zoom_selection();
                ui.close_menu();
            }
        }
        if ui.button("Mostrar Tudo (Fit All)").clicked() {
            doc.zoom_fit();
            ui.close_menu();
        }
    });

    play_request
}

fn draw_overview_bar(
    ui: &mut Ui,
    doc: &mut AudioDocument,
    interaction: &mut WaveformInteraction,
) {
    let avail_w = ui.available_width().max(100.0);
    let (rect, response) = ui.allocate_exact_size(Vec2::new(avail_w, 34.0), Sense::click_and_drag());
    let painter = ui.painter_at(rect);

    painter.rect_filled(rect, 2.0, ForgeTheme::CANVAS_BG);
    painter.rect_stroke(rect, 2.0, Stroke::new(1.0, ForgeTheme::BORDER));

    let total = doc.buffer.num_frames();
    if total == 0 {
        return;
    }

    // Handle click/drag to pan viewport
    if response.drag_started_by(egui::PointerButton::Primary)
        || response.clicked_by(egui::PointerButton::Primary)
    {
        interaction.overview_dragging = true;
    }
    if response.drag_stopped() {
        interaction.overview_dragging = false;
    }
    if (interaction.overview_dragging || response.clicked_by(egui::PointerButton::Primary))
        && (response.dragged() || response.clicked())
    {
        if let Some(pos) = response.interact_pointer_pos() {
            let ratio = ((pos.x - rect.min.x) / rect.width().max(1.0)).clamp(0.0, 1.0) as f64;
            let center_frame = ratio * total as f64;
            doc.view_start = center_frame - doc.view_frames * 0.5;
            doc.clamp_viewport();
        }
    }

    // Draw miniature mono overview waveform
    let px_w = rect.width().ceil().max(1.0) as usize;
    let frames_per_px = total as f64 / px_w as f64;
    let mid_y = rect.center().y;
    let half_h = rect.height() * 0.42;

    for px in 0..px_w {
        let f0 = (px as f64 * frames_per_px).floor() as usize;
        let f1 = (((px + 1) as f64 * frames_per_px).ceil() as usize).clamp(f0 + 1, total);
        let (min_v, max_v) = doc.pyramid.query_range(&doc.buffer, 0, f0, f1);
        let x = rect.min.x + px as f32 + 0.5;
        painter.line_segment(
            [
                Pos2::new(x, mid_y - max_v.clamp(-1.0, 1.0) * half_h),
                Pos2::new(x, mid_y - min_v.clamp(-1.0, 1.0) * half_h),
            ],
            Stroke::new(1.0, Color32::from_rgb(45, 135, 95)),
        );
    }

    // Draw markers as little ticks
    for m in &doc.annotations.markers {
        let x = rect.min.x + (m.frame as f32 / total as f32) * rect.width();
        painter.line_segment(
            [Pos2::new(x, rect.min.y), Pos2::new(x, rect.max.y)],
            Stroke::new(1.0, ForgeTheme::MARKER),
        );
    }

    // Draw current viewport window highlight
    let vx0 = rect.min.x + (doc.view_start as f32 / total as f32) * rect.width();
    let vx1 = rect.min.x + ((doc.view_start + doc.view_frames) as f32 / total as f32) * rect.width();
    let vp_rect = Rect::from_min_max(
        Pos2::new(vx0.max(rect.min.x), rect.min.y + 1.0),
        Pos2::new(vx1.min(rect.max.x).max(vx0 + 4.0), rect.max.y - 1.0),
    );
    painter.rect_filled(
        vp_rect,
        1.0,
        Color32::from_rgba_premultiplied(220, 235, 255, 28),
    );
    painter.rect_stroke(
        vp_rect,
        1.0,
        Stroke::new(1.5, Color32::from_rgb(160, 200, 245)),
    );
}

fn draw_time_ruler(
    painter: &egui::Painter,
    rect: Rect,
    view_start: f64,
    view_end: f64,
    sr: f64,
    frame_to_x: &dyn Fn(f64) -> f32,
) {
    let start_secs = view_start / sr;
    let end_secs = view_end / sr;
    let span_secs = (end_secs - start_secs).max(1e-4);

    let steps = [
        0.005, 0.01, 0.02, 0.05, 0.1, 0.25, 0.5, 1.0, 2.0, 5.0, 10.0, 15.0, 30.0, 60.0, 120.0,
        300.0,
    ];
    let target_ticks = (rect.width() / 95.0).max(2.0) as f64;
    let raw_step = span_secs / target_ticks;
    let step = steps
        .iter()
        .copied()
        .find(|&s| s >= raw_step)
        .unwrap_or(600.0);

    let first_tick = (start_secs / step).floor() * step;
    let mut t = first_tick;
    let ruler_painter = painter.with_clip_rect(rect);

    while t <= end_secs + step * 0.1 {
        let x = frame_to_x(t * sr);
        if x >= rect.min.x - 2.0 && x <= rect.max.x + 2.0 {
            ruler_painter.line_segment(
                [Pos2::new(x, rect.max.y - 8.0), Pos2::new(x, rect.max.y)],
                Stroke::new(1.0, ForgeTheme::TEXT_MUTED),
            );
            ruler_painter.text(
                Pos2::new(x + 3.0, rect.min.y + 3.0),
                Align2::LEFT_TOP,
                format_timecode(t),
                FontId::monospace(10.0),
                ForgeTheme::TEXT_MUTED,
            );
        }
        t += step;
    }
}

fn draw_db_ruler(painter: &egui::Painter, rect: Rect, mid_y: f32, half_h: f32) {
    let marks = [
        (1.0_f32, "0"),
        (0.5_f32, "-6"),
        (0.25_f32, "-12"),
        (0.0_f32, "-∞"),
    ];
    let p = painter.with_clip_rect(rect);
    for (amp, label) in marks {
        if amp == 0.0 {
            p.text(
                Pos2::new(rect.min.x + 4.0, mid_y),
                Align2::LEFT_CENTER,
                label,
                FontId::monospace(9.5),
                ForgeTheme::TEXT_MUTED,
            );
        } else {
            let y_up = mid_y - amp * half_h;
            let y_dn = mid_y + amp * half_h;
            p.text(
                Pos2::new(rect.min.x + 4.0, y_up),
                Align2::LEFT_CENTER,
                label,
                FontId::monospace(9.5),
                ForgeTheme::TEXT_MUTED,
            );
            if y_dn < rect.max.y - 6.0 {
                p.text(
                    Pos2::new(rect.min.x + 4.0, y_dn),
                    Align2::LEFT_CENTER,
                    label,
                    FontId::monospace(9.5),
                    ForgeTheme::TEXT_MUTED,
                );
            }
        }
    }
}

/// Draws a classic Sound Forge-style vertical Stereo VU Meter.
pub fn draw_vu_meter(ui: &mut Ui, meter: &MeterReading) {
    ui.vertical(|ui| {
        ui.label(
            egui::RichText::new("NÍVEL (dB)")
                .size(10.5)
                .color(ForgeTheme::TEXT_MUTED),
        );
        let avail_h = (ui.available_height() - 28.0).clamp(120.0, 420.0);
        let (rect, _) = ui.allocate_exact_size(Vec2::new(56.0, avail_h), Sense::hover());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 3.0, ForgeTheme::CANVAS_BG);
        painter.rect_stroke(rect, 3.0, Stroke::new(1.0, ForgeTheme::BORDER));

        let peaks = [meter.peak_l, meter.peak_r];
        let bar_w = 16.0;
        let pad_top = 8.0;
        let pad_bot = 16.0;
        let bar_h = (rect.height() - pad_top - pad_bot).max(20.0);
        let segments = 24;

        for (ch, &peak) in peaks.iter().enumerate() {
            let x0 = rect.min.x + 8.0 + ch as f32 * (bar_w + 8.0);
            let db = amp_to_db(peak).clamp(-60.0, 0.0);
            let norm = ((db + 60.0) / 60.0).clamp(0.0, 1.0);
            let active_segs = (norm * segments as f32).round() as usize;

            for seg in 0..segments {
                let frac = (seg + 1) as f32 / segments as f32;
                let seg_h = bar_h / segments as f32;
                let y1 = rect.max.y - pad_bot - seg as f32 * seg_h;
                let y0 = y1 - (seg_h - 1.5).max(1.0);
                let seg_rect = Rect::from_min_max(Pos2::new(x0, y0), Pos2::new(x0 + bar_w, y1));

                let base_color = if frac > 0.88 {
                    ForgeTheme::ACCENT_RED
                } else if frac > 0.72 {
                    ForgeTheme::ACCENT_AMBER
                } else {
                    ForgeTheme::ACCENT_GREEN
                };

                let color = if seg < active_segs {
                    base_color
                } else {
                    Color32::from_rgba_premultiplied(
                        base_color.r() / 5,
                        base_color.g() / 5,
                        base_color.b() / 5,
                        255,
                    )
                };
                painter.rect_filled(seg_rect, 1.0, color);
            }

            painter.text(
                Pos2::new(x0 + bar_w * 0.5, rect.max.y - 4.0),
                Align2::CENTER_BOTTOM,
                if ch == 0 { "L" } else { "R" },
                FontId::monospace(10.0),
                ForgeTheme::TEXT_MUTED,
            );
        }

        let peak_db = amp_to_db(meter.peak_l.max(meter.peak_r));
        let db_text = if peak_db <= -90.0 {
            "-∞ dB".to_string()
        } else {
            format!("{peak_db:.1} dB")
        };
        ui.label(egui::RichText::new(db_text).monospace().size(11.0));
    });
}
