use crate::document::AudioDocument;
use crate::playback::MeterReading;
use crate::theme::{amp_to_db, format_ruler_time, format_timecode, ForgeTheme, Palette, UiSkin};
use egui::{Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, Ui, Vec2};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActiveTool {
    /// Standard I-Beam edit/selection tool
    EditSelect,
    /// Magnify / Zoom tool
    MagnifyZoom,
    /// Pencil sample-draw tool (repairs clicks when zoomed in)
    PencilDraw,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DocWindowAction {
    None,
    Activate,
    PlayFrom(usize),
    PlayWindow,
    StopWindow,
    ToggleLoopPlay,
    ToggleMaximize,
    CloseWindow,
    SendSelectionToOtherDoc,
}

pub struct WaveformInteraction {
    pub drag_doc_id: Option<usize>,
    pub drag_anchor_frame: Option<usize>,
    pub overview_dragging_doc: Option<usize>,
    pub pencil_undo_pushed: bool,
}

impl Default for WaveformInteraction {
    fn default() -> Self {
        Self {
            drag_doc_id: None,
            drag_anchor_frame: None,
            overview_dragging_doc: None,
            pencil_undo_pushed: false,
        }
    }
}

/// Renders a complete Sound Forge Pro 10.0 MDI Audio Window (Titlebar + Overview + Left Channel Strip + Waveform + Bottom Mini-Transport & Status Boxes).
pub fn draw_sound_forge_doc_window(
    ui: &mut Ui,
    doc: &mut AudioDocument,
    is_active: bool,
    is_maximized: bool,
    has_multiple_docs: bool,
    window_height: f32,
    interaction: &mut WaveformInteraction,
    playhead_frame: Option<usize>,
    is_playing_this_doc: bool,
    loop_enabled: bool,
    snap_zero_crossing: bool,
    active_tool: ActiveTool,
    skin: UiSkin,
) -> DocWindowAction {
    let mut action = DocWindowAction::None;
    let p = skin.palette();

    let outer_frame = egui::Frame::none()
        .fill(p.panel_bg)
        .stroke(Stroke::new(
            if is_active { 1.5_f32 } else { 1.0_f32 },
            if is_active {
                p.selection_border
            } else {
                p.border
            },
        ))
        .rounding(egui::Rounding::same(3.0));

    outer_frame.show(ui, |ui| {
        // 1. Classic Sound Forge MDI Window Titlebar
        let title_bg = if is_active {
            p.win_title_active
        } else {
            p.win_title_inactive
        };
        egui::Frame::none()
            .fill(title_bg)
            .inner_margin(egui::Margin::symmetric(6.0, 3.0))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    let icon_resp = ui.add(
                        egui::Label::new(
                            egui::RichText::new("🟥")
                                .size(12.0)
                                .color(Color32::from_rgb(200, 40, 40)),
                        )
                        .sense(Sense::click()),
                    );
                    let title_resp = ui.add(
                        egui::Label::new(
                            egui::RichText::new(doc.formatted_title_with_duration())
                                .strong()
                                .size(12.5)
                                .color(p.win_title_text),
                        )
                        .sense(Sense::click()),
                    );
                    if icon_resp.clicked() || title_resp.clicked() {
                        action = DocWindowAction::Activate;
                    }
                    if title_resp.double_clicked() {
                        action = DocWindowAction::ToggleMaximize;
                    }

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .add(
                                egui::Button::new(
                                    egui::RichText::new("✕").size(11.0).color(Color32::WHITE),
                                )
                                .fill(Color32::from_rgb(196, 56, 48))
                                .min_size(Vec2::new(22.0, 18.0)),
                            )
                            .on_hover_text("Fechar música")
                            .clicked()
                        {
                            action = DocWindowAction::CloseWindow;
                        }

                        let max_lbl = if is_maximized { "❐" } else { "□" };
                        if ui
                            .add(
                                egui::Button::new(egui::RichText::new(max_lbl).size(11.0))
                                    .min_size(Vec2::new(22.0, 18.0)),
                            )
                            .on_hover_text("Maximizar / Restaurar janela da música")
                            .clicked()
                        {
                            action = DocWindowAction::ToggleMaximize;
                        }

                        let min_lbl = if doc.minimized { "▲" } else { "—" };
                        if ui
                            .add(
                                egui::Button::new(egui::RichText::new(min_lbl).size(11.0))
                                    .min_size(Vec2::new(22.0, 18.0)),
                            )
                            .on_hover_text("Minimizar / Expandir faixa")
                            .clicked()
                        {
                            doc.minimized = !doc.minimized;
                            action = DocWindowAction::Activate;
                        }

                        if has_multiple_docs && doc.selection.is_some() {
                            if ui
                                .button("📥 Enviar Trecho p/ Outra Música")
                                .on_hover_text(
                                    "Envia o trecho selecionado desta música diretamente para outra música aberta",
                                )
                                .clicked()
                            {
                                action = DocWindowAction::SendSelectionToOtherDoc;
                            }
                        }
                    });
                });
            });

        if doc.minimized {
            return;
        }

        // 2. Overview Bar + Marker Strip + Waveform + Left Channel Header
        let body_h = (window_height - 58.0).max(115.0);
        let avail_w = ui.available_width().max(240.0);
        let (body_rect, body_resp) =
            ui.allocate_exact_size(Vec2::new(avail_w, body_h), Sense::click_and_drag());

        if body_resp.clicked() || body_resp.drag_started() {
            action = DocWindowAction::Activate;
        }

        let left_strip_w = 48.0;
        let overview_h = 18.0;
        let marker_h = 16.0;
        let ruler_h = 18.0;
        let header_total_h = overview_h + marker_h + ruler_h;

        let left_header_rect = Rect::from_min_max(
            body_rect.min,
            Pos2::new(body_rect.min.x + left_strip_w, body_rect.min.y + header_total_h),
        );
        let overview_rect = Rect::from_min_max(
            Pos2::new(body_rect.min.x + left_strip_w, body_rect.min.y),
            Pos2::new(body_rect.max.x, body_rect.min.y + overview_h),
        );
        let marker_rect = Rect::from_min_max(
            Pos2::new(body_rect.min.x + left_strip_w, overview_rect.max.y),
            Pos2::new(body_rect.max.x, overview_rect.max.y + marker_h),
        );
        let ruler_rect = Rect::from_min_max(
            Pos2::new(body_rect.min.x + left_strip_w, marker_rect.max.y),
            Pos2::new(body_rect.max.x, marker_rect.max.y + ruler_h),
        );
        let left_ch_rect = Rect::from_min_max(
            Pos2::new(body_rect.min.x, ruler_rect.max.y),
            Pos2::new(body_rect.min.x + left_strip_w, body_rect.max.y),
        );
        let wave_rect = Rect::from_min_max(
            Pos2::new(body_rect.min.x + left_strip_w, ruler_rect.max.y),
            body_rect.max,
        );

        let painter = ui.painter_at(body_rect);
        painter.rect_filled(left_header_rect, 0.0, p.ruler_bg);
        painter.rect_filled(overview_rect, 0.0, p.toolbar_bg);
        painter.rect_filled(marker_rect, 0.0, Color32::from_rgb(206, 210, 216));
        painter.rect_filled(ruler_rect, 0.0, p.ruler_bg);
        painter.rect_filled(left_ch_rect, 0.0, p.ruler_bg);
        painter.rect_filled(wave_rect, 0.0, p.canvas_bg);
        painter.rect_stroke(body_rect, 0.0, Stroke::new(1.0_f32, p.border));

        // Draw padlock & tool icon in left_header_rect (like Screenshot 2!)
        painter.text(
            Pos2::new(left_header_rect.min.x + 6.0, left_header_rect.center().y + 4.0),
            Align2::LEFT_CENTER,
            "🔒 ┼",
            FontId::proportional(11.5),
            p.text_muted,
        );

        let total_frames = doc.buffer.num_frames();
        if total_frames == 0 {
            painter.text(
                wave_rect.center(),
                Align2::CENTER_CENTER,
                "Sem áudio carregado",
                FontId::proportional(13.0),
                p.text_muted,
            );
            return;
        }

        // Overview bar interaction & rendering
        draw_window_overview(
            &painter,
            overview_rect,
            &body_resp,
            doc,
            interaction,
            playhead_frame,
            &p,
        );

        // Waveform & Ruler interactions
        handle_waveform_interactions(
            ui,
            &body_resp,
            wave_rect,
            ruler_rect,
            left_ch_rect,
            doc,
            interaction,
            snap_zero_crossing,
            active_tool,
        );

        let sr = doc.buffer.sample_rate as f64;
        let view_start = doc.view_start;
        let view_frames = doc.view_frames.max(1.0);
        let view_end = view_start + view_frames;
        let frame_to_x = |f: f64| -> f32 {
            let ratio = ((f - view_start) / view_frames) as f32;
            wave_rect.min.x + ratio * wave_rect.width()
        };

        // Yellow start triangle on marker_rect (Screenshot 2!)
        if view_start <= 1.0 {
            let x0 = wave_rect.min.x;
            painter.add(egui::Shape::convex_polygon(
                vec![
                    Pos2::new(x0, marker_rect.min.y + 1.0),
                    Pos2::new(x0 + 9.0, marker_rect.min.y + 1.0),
                    Pos2::new(x0, marker_rect.max.y - 1.0),
                ],
                Color32::from_rgb(245, 205, 20),
                Stroke::new(1.0_f32, Color32::from_rgb(140, 110, 10)),
            ));
        }

        // Time ruler
        draw_time_ruler(&painter, ruler_rect, view_start, view_end, sr, &frame_to_x, &p);

        // Channels + Left Channel Badges (`[1] [-]` and `[2] [-]` + `-Inf.`)
        let channels = doc.buffer.channels.max(1);
        let chan_h = wave_rect.height() / channels as f32;
        let wave_painter = painter.with_clip_rect(wave_rect);

        for ch in 0..channels {
            let y0 = wave_rect.min.y + ch as f32 * chan_h;
            let y1 = y0 + chan_h;
            let ch_rect = Rect::from_min_max(Pos2::new(wave_rect.min.x, y0), Pos2::new(wave_rect.max.x, y1));
            let strip_rect = Rect::from_min_max(Pos2::new(left_ch_rect.min.x, y0), Pos2::new(left_ch_rect.max.x, y1));

            let ch_enabled = doc.channel_enabled.get(ch).copied().unwrap_or(true);
            if !ch_enabled {
                wave_painter.rect_filled(
                    ch_rect,
                    0.0,
                    Color32::from_rgba_premultiplied(150, 150, 150, 45),
                );
            }

            // Left channel strip badge `[1] [-]` / `[2] [-]`
            let badge_rect = Rect::from_min_size(
                Pos2::new(strip_rect.min.x + 3.0, strip_rect.min.y + 4.0),
                Vec2::new(16.0, 15.0),
            );
            let minus_rect = Rect::from_min_size(
                Pos2::new(strip_rect.min.x + 21.0, strip_rect.min.y + 4.0),
                Vec2::new(13.0, 15.0),
            );
            let badge_col = if ch_enabled {
                p.channel_badge_bg
            } else {
                Color32::from_rgb(130, 135, 145)
            };
            painter.rect_filled(badge_rect, 2.0, badge_col);
            painter.text(
                badge_rect.center(),
                Align2::CENTER_CENTER,
                format!("{}", ch + 1),
                FontId::monospace(10.5),
                Color32::WHITE,
            );
            painter.rect_filled(minus_rect, 2.0, badge_col);
            painter.text(
                minus_rect.center(),
                Align2::CENTER_CENTER,
                "-",
                FontId::monospace(11.0),
                Color32::WHITE,
            );

            let mid_y = ch_rect.center().y;
            let half_h = (ch_rect.height() * 0.45).max(4.0);

            // `-Inf.` label at zero line on the left strip (Screenshot 1 & 2!)
            painter.text(
                Pos2::new(strip_rect.max.x - 3.0, mid_y),
                Align2::RIGHT_CENTER,
                "-Inf.",
                FontId::proportional(9.5),
                p.text_main,
            );
            painter.line_segment(
                [
                    Pos2::new(strip_rect.max.x - 1.0, strip_rect.min.y),
                    Pos2::new(strip_rect.max.x - 1.0, strip_rect.max.y),
                ],
                Stroke::new(1.0_f32, p.border),
            );

            // Subtle top/bottom peak boundary lines & zero line
            wave_painter.line_segment(
                [
                    Pos2::new(ch_rect.min.x, mid_y - half_h),
                    Pos2::new(ch_rect.max.x, mid_y - half_h),
                ],
                Stroke::new(1.0_f32, p.grid_major),
            );
            wave_painter.line_segment(
                [
                    Pos2::new(ch_rect.min.x, mid_y + half_h),
                    Pos2::new(ch_rect.max.x, mid_y + half_h),
                ],
                Stroke::new(1.0_f32, p.grid_major),
            );
            wave_painter.line_segment(
                [Pos2::new(ch_rect.min.x, mid_y), Pos2::new(ch_rect.max.x, mid_y)],
                Stroke::new(1.0_f32, p.grid_zero),
            );

            if ch + 1 < channels {
                painter.line_segment(
                    [
                        Pos2::new(body_rect.min.x, ch_rect.max.y),
                        Pos2::new(body_rect.max.x, ch_rect.max.y),
                    ],
                    Stroke::new(1.0_f32, p.border),
                );
            }

            // Draw waveform peaks (Deep Navy Blue in Classic Sound Forge 10!)
            let px_width = wave_rect.width().ceil().max(1.0) as usize;
            let frames_per_px = view_frames / px_width as f64;
            let wave_col = if ch_enabled {
                p.wave_fill
            } else {
                Color32::from_rgb(130, 140, 165)
            };

            if frames_per_px <= 1.2 {
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
                    wave_painter.add(egui::Shape::line(pts, Stroke::new(1.5_f32, wave_col)));
                }
            } else {
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
                        Stroke::new(1.0_f32, wave_col),
                    );
                }
            }
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
                wave_painter.rect_filled(r_rect, 0.0, p.region_fill);
                wave_painter.line_segment(
                    [Pos2::new(x0, wave_rect.min.y), Pos2::new(x0, wave_rect.max.y)],
                    Stroke::new(1.0_f32, p.region_border),
                );
                wave_painter.line_segment(
                    [Pos2::new(x1, wave_rect.min.y), Pos2::new(x1, wave_rect.max.y)],
                    Stroke::new(1.0_f32, p.region_border),
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
                wave_painter.rect_filled(sel_rect, 0.0, p.selection_fill);
                wave_painter.line_segment(
                    [Pos2::new(x0, wave_rect.min.y), Pos2::new(x0, wave_rect.max.y)],
                    Stroke::new(1.5_f32, p.selection_border),
                );
                wave_painter.line_segment(
                    [Pos2::new(x1, wave_rect.min.y), Pos2::new(x1, wave_rect.max.y)],
                    Stroke::new(1.5_f32, p.selection_border),
                );
            }
        }

        // Draw Orange Numbered Markers (`1`, `2`, `3` in marker_rect + orange vertical line in wave_rect, like Screenshot 1!)
        let marker_painter = painter.with_clip_rect(marker_rect);
        for (idx, m) in doc.annotations.markers.iter().enumerate() {
            let x = frame_to_x(m.frame as f64);
            if x >= wave_rect.min.x - 12.0 && x <= wave_rect.max.x + 12.0 {
                wave_painter.line_segment(
                    [Pos2::new(x, ruler_rect.min.y), Pos2::new(x, wave_rect.max.y)],
                    Stroke::new(1.2_f32, p.marker),
                );
                let sq = Rect::from_min_size(
                    Pos2::new(x, marker_rect.min.y + 1.0),
                    Vec2::new(15.0, marker_rect.height() - 2.0),
                );
                marker_painter.rect_filled(sq, 1.5, p.marker);
                marker_painter.text(
                    sq.center(),
                    Align2::CENTER_CENTER,
                    format!("{}", idx + 1),
                    FontId::proportional(10.0),
                    Color32::WHITE,
                );
            }
        }

        // Draw Edit Cursor
        let cur_x = frame_to_x(doc.cursor_frame as f64);
        if cur_x >= wave_rect.min.x && cur_x <= wave_rect.max.x {
            wave_painter.line_segment(
                [Pos2::new(cur_x, wave_rect.min.y), Pos2::new(cur_x, wave_rect.max.y)],
                Stroke::new(1.2_f32, p.cursor),
            );
        }

        // Draw Live Playhead
        if let Some(pf) = playhead_frame {
            let px = frame_to_x(pf as f64);
            if px >= wave_rect.min.x && px <= wave_rect.max.x {
                wave_painter.line_segment(
                    [Pos2::new(px, wave_rect.min.y), Pos2::new(px, wave_rect.max.y)],
                    Stroke::new(2.0_f32, p.playhead),
                );
            }
        }

        // Right-click context menu on waveform
        body_resp.context_menu(|ui| {
            if ui.button("▶ Tocar daqui").clicked() {
                action = DocWindowAction::PlayFrom(doc.cursor_frame);
                ui.close_menu();
            }
            if ui.button("🚩 Inserir Marcador aqui (M)").clicked() {
                doc.annotations.add_marker(doc.cursor_frame, None);
                ui.close_menu();
            }
            if let Some((s, e)) = doc.selection {
                if ui.button("📏 Criar Região da Seleção (R)").clicked() {
                    doc.annotations.add_region(s, e, None);
                    ui.close_menu();
                }
                if has_multiple_docs {
                    if ui.button("📥 Enviar Seleção para Outra Música...").clicked() {
                        action = DocWindowAction::SendSelectionToOtherDoc;
                        ui.close_menu();
                    }
                }
                if ui.button("🔍 Zoom na Seleção").clicked() {
                    doc.zoom_selection();
                    ui.close_menu();
                }
            }
            if ui.button("↔ Mostrar Música Inteira (Fit All)").clicked() {
                doc.zoom_fit();
                ui.close_menu();
            }
        });

        // 3. Per-Window Bottom Mini-Transport + Rate Slider + 3 Sunken Status Boxes (Screenshot 1!)
        egui::Frame::none()
            .fill(p.toolbar_bg)
            .inner_margin(egui::Margin::symmetric(4.0, 2.0))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    if ui
                        .small_button("|◀")
                        .on_hover_text("Ir para o início")
                        .clicked()
                    {
                        doc.cursor_frame = 0;
                        action = DocWindowAction::Activate;
                    }
                    if ui
                        .small_button("◀◀")
                        .on_hover_text("Retroceder 2 segundos")
                        .clicked()
                    {
                        let step = doc.buffer.sample_rate as usize * 2;
                        doc.cursor_frame = doc.cursor_frame.saturating_sub(step);
                        action = DocWindowAction::Activate;
                    }
                    if ui.small_button("■").on_hover_text("Parar").clicked() {
                        action = DocWindowAction::StopWindow;
                    }
                    let play_txt = if is_playing_this_doc { "⏸" } else { "▶" };
                    if ui
                        .small_button(play_txt)
                        .on_hover_text("Tocar esta música")
                        .clicked()
                    {
                        action = DocWindowAction::PlayWindow;
                    }
                    let loop_btn = egui::Button::new(
                        egui::RichText::new("🔁").size(11.0).color(if loop_enabled {
                            ForgeTheme::ACCENT_GREEN
                        } else {
                            p.text_main
                        }),
                    );
                    if ui
                        .add(loop_btn)
                        .on_hover_text("Tocar em Loop")
                        .clicked()
                    {
                        action = DocWindowAction::ToggleLoopPlay;
                    }

                    ui.separator();
                    ui.label(
                        egui::RichText::new("┼")
                            .color(ForgeTheme::ACCENT_GREEN)
                            .strong(),
                    );
                    if ui
                        .add(
                            egui::Label::new(
                                egui::RichText::new(format!("Rate: {:+.2}", doc.rate_slider))
                                    .size(11.0),
                            )
                            .sense(Sense::click()),
                        )
                        .on_hover_text("Clique para zerar Rate (0,00)")
                        .clicked()
                    {
                        doc.rate_slider = 0.0;
                    }
                    ui.add(
                        egui::Slider::new(&mut doc.rate_slider, -12.0..=12.0)
                            .show_value(false)
                            .trailing_fill(false),
                    )
                    .on_hover_text("Controle de Rate / Pitch desta janela (-12 a +12 semitons)");

                    // Right-aligned 3 sunken status boxes (Start, End, Zoom Ratio — Screenshot 1!)
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let zoom_ratio = (doc.view_frames / wave_rect.width().max(1.0) as f64)
                            .round()
                            .max(1.0) as usize;
                        draw_sunken_box(ui, &format!("1:{zoom_ratio}"), 64.0, &p);

                        let (box1, box2) = if let Some((s, e)) = doc.selection {
                            (
                                format_timecode(doc.buffer.frame_to_secs(s)),
                                format_timecode(doc.buffer.frame_to_secs(e)),
                            )
                        } else {
                            (
                                format_timecode(doc.buffer.frame_to_secs(doc.cursor_frame)),
                                format_timecode(doc.buffer.duration_secs()),
                            )
                        };
                        draw_sunken_box(ui, &box2, 86.0, &p);
                        draw_sunken_box(ui, &box1, 86.0, &p);
                    });
                });
            });
    });

    action
}

fn draw_sunken_box(ui: &mut Ui, text: &str, width: f32, p: &Palette) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, 18.0), Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 1.0, p.canvas_bg);
    painter.rect_stroke(rect, 1.0, Stroke::new(1.0_f32, p.border));
    painter.text(
        Pos2::new(rect.max.x - 4.0, rect.center().y),
        Align2::RIGHT_CENTER,
        text,
        FontId::monospace(10.0),
        p.text_main,
    );
}

fn draw_window_overview(
    painter: &egui::Painter,
    rect: Rect,
    body_resp: &egui::Response,
    doc: &mut AudioDocument,
    interaction: &mut WaveformInteraction,
    playhead_frame: Option<usize>,
    p: &Palette,
) {
    let total = doc.buffer.num_frames().max(1);

    if body_resp.drag_started_by(egui::PointerButton::Primary)
        || body_resp.clicked_by(egui::PointerButton::Primary)
    {
        if let Some(pos) = body_resp.interact_pointer_pos() {
            if rect.contains(pos) {
                interaction.overview_dragging_doc = Some(doc.id);
            }
        }
    }
    if body_resp.drag_stopped() {
        interaction.overview_dragging_doc = None;
    }
    if interaction.overview_dragging_doc == Some(doc.id) {
        if let Some(pos) = body_resp.interact_pointer_pos() {
            let ratio = ((pos.x - rect.min.x) / rect.width().max(1.0)).clamp(0.0, 1.0) as f64;
            let center_frame = ratio * total as f64;
            doc.view_start = center_frame - doc.view_frames * 0.5;
            doc.clamp_viewport();
        }
    }

    // Viewport thumb inside overview bar
    let vx0 = rect.min.x + (doc.view_start as f32 / total as f32) * rect.width();
    let vx1 = rect.min.x + ((doc.view_start + doc.view_frames) as f32 / total as f32) * rect.width();
    let thumb = Rect::from_min_max(
        Pos2::new(vx0.max(rect.min.x), rect.min.y + 2.0),
        Pos2::new(vx1.min(rect.max.x).max(vx0 + 8.0), rect.max.y - 2.0),
    );
    painter.rect_filled(thumb, 1.0, p.canvas_bg);
    painter.rect_stroke(thumb, 1.0, Stroke::new(1.0_f32, p.border));

    // Cursor / playhead tick inside overview bar (like Screenshot 1!)
    let cur_f = playhead_frame.unwrap_or(doc.cursor_frame);
    let cx = rect.min.x + (cur_f as f32 / total as f32) * rect.width();
    painter.line_segment(
        [Pos2::new(cx, rect.min.y + 2.0), Pos2::new(cx, rect.max.y - 2.0)],
        Stroke::new(1.5_f32, p.cursor),
    );
}

fn handle_waveform_interactions(
    ui: &Ui,
    response: &egui::Response,
    wave_rect: Rect,
    ruler_rect: Rect,
    left_ch_rect: Rect,
    doc: &mut AudioDocument,
    interaction: &mut WaveformInteraction,
    snap_zero_crossing: bool,
    active_tool: ActiveTool,
) {
    let total_frames = doc.buffer.num_frames();
    if total_frames == 0 {
        return;
    }
    let view_start = doc.view_start;
    let view_frames = doc.view_frames.max(1.0);
    let x_to_frame = |x: f32| -> usize {
        let ratio = ((x - wave_rect.min.x) / wave_rect.width().max(1.0)).clamp(0.0, 1.0) as f64;
        let f = (view_start + ratio * view_frames).round() as usize;
        f.min(total_frames)
    };

    // Click on left channel badge `[1]` / `[2]` to toggle channel enable!
    if response.clicked_by(egui::PointerButton::Primary) {
        if let Some(pos) = response.interact_pointer_pos() {
            if left_ch_rect.contains(pos) && doc.buffer.channels >= 2 {
                let ch = if pos.y < left_ch_rect.center().y { 0 } else { 1 };
                doc.channel_enabled[ch] = !doc.channel_enabled[ch];
                if !doc.channel_enabled[0] && !doc.channel_enabled[1] {
                    doc.channel_enabled[1 - ch] = true;
                }
                return;
            }
        }
    }

    // Mouse wheel zoom & pan
    if response.hovered() {
        let scroll = ui.input(|i| i.smooth_scroll_delta);
        let shift = ui.input(|i| i.modifiers.shift);
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
        }
    }

    let middle_down = ui.input(|i| i.pointer.middle_down());
    if response.dragged_by(egui::PointerButton::Middle) || (middle_down && response.dragged()) {
        let dx = response.drag_delta().x;
        let pan_frames = -(dx as f64 / wave_rect.width().max(1.0) as f64) * view_frames;
        doc.view_start += pan_frames;
        doc.clamp_viewport();
        return;
    }

    // Pencil Tool: draw directly on samples when zoomed in
    if active_tool == ActiveTool::PencilDraw {
        if response.drag_started_by(egui::PointerButton::Primary) {
            if let Some(pos) = response.interact_pointer_pos() {
                if wave_rect.contains(pos) {
                    doc.push_undo();
                    interaction.pencil_undo_pushed = true;
                }
            }
        }
        if response.dragged_by(egui::PointerButton::Primary) && interaction.pencil_undo_pushed {
            if let Some(pos) = response.interact_pointer_pos() {
                if wave_rect.contains(pos) {
                    let f = x_to_frame(pos.x).min(total_frames.saturating_sub(1));
                    let channels = doc.buffer.channels.max(1);
                    let chan_h = wave_rect.height() / channels as f32;
                    let ch = (((pos.y - wave_rect.min.y) / chan_h).floor() as usize)
                        .min(channels - 1);
                    let ch_mid_y = wave_rect.min.y + (ch as f32 + 0.5) * chan_h;
                    let half_h = (chan_h * 0.45).max(4.0);
                    let val = ((ch_mid_y - pos.y) / half_h).clamp(-1.0, 1.0);
                    let mut cloned = (*doc.buffer).clone();
                    cloned.set_sample(f, ch, val);
                    doc.buffer = std::sync::Arc::new(cloned);
                    doc.dirty = true;
                }
            }
        }
        if response.drag_stopped() && interaction.pencil_undo_pushed {
            doc.pyramid = crate::audio::WaveformPyramid::build(&doc.buffer);
            interaction.pencil_undo_pushed = false;
        }
        return;
    }

    // EditSelect or MagnifyZoom tool
    if response.drag_started_by(egui::PointerButton::Primary) {
        if let Some(pos) = response.interact_pointer_pos() {
            if wave_rect.contains(pos) || ruler_rect.contains(pos) {
                let mut f = x_to_frame(pos.x);
                if snap_zero_crossing && active_tool == ActiveTool::EditSelect {
                    let search = (doc.buffer.sample_rate as usize / 80).max(32);
                    f = doc.buffer.find_nearest_zero_crossing(f, search);
                }
                interaction.drag_doc_id = Some(doc.id);
                interaction.drag_anchor_frame = Some(f);
                doc.cursor_frame = f;
                doc.selection = None;
            }
        }
    } else if response.dragged_by(egui::PointerButton::Primary)
        && interaction.drag_doc_id == Some(doc.id)
    {
        if let (Some(anchor), Some(pos)) = (
            interaction.drag_anchor_frame,
            response.interact_pointer_pos(),
        ) {
            let mut f = x_to_frame(pos.x);
            if snap_zero_crossing && active_tool == ActiveTool::EditSelect {
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
    } else if response.drag_stopped() && interaction.drag_doc_id == Some(doc.id) {
        if active_tool == ActiveTool::MagnifyZoom {
            if doc.selection.is_some() {
                doc.zoom_selection();
                doc.selection = None;
            } else {
                doc.zoom_at(0.5, 0.5);
            }
        }
        interaction.drag_doc_id = None;
        interaction.drag_anchor_frame = None;
    } else if response.clicked_by(egui::PointerButton::Primary) {
        if let Some(pos) = response.interact_pointer_pos() {
            if wave_rect.contains(pos) || ruler_rect.contains(pos) {
                if active_tool == ActiveTool::MagnifyZoom {
                    let anchor = ((pos.x - wave_rect.min.x) / wave_rect.width().max(1.0)) as f64;
                    let alt = ui.input(|i| i.modifiers.alt || i.modifiers.shift);
                    doc.zoom_at(if alt { 2.0 } else { 0.5 }, anchor);
                    return;
                }
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
}

fn draw_time_ruler(
    painter: &egui::Painter,
    rect: Rect,
    view_start: f64,
    view_end: f64,
    sr: f64,
    frame_to_x: &dyn Fn(f64) -> f32,
    p: &Palette,
) {
    let start_secs = view_start / sr;
    let end_secs = view_end / sr;
    let span_secs = (end_secs - start_secs).max(1e-4);

    let steps = [
        0.005, 0.01, 0.02, 0.05, 0.1, 0.25, 0.5, 1.0, 2.0, 5.0, 10.0, 15.0, 30.0, 60.0, 120.0,
        300.0,
    ];
    let target_ticks = (rect.width() / 90.0).max(2.0) as f64;
    let raw_step = span_secs / target_ticks;
    let step = steps
        .iter()
        .copied()
        .find(|&s| s >= raw_step)
        .unwrap_or(600.0);

    let first_tick = (start_secs / step).floor() * step;
    let mut t = first_tick;
    let ruler_painter = painter.with_clip_rect(rect);

    // Minor ticks (5 subdivisions per major step)
    let sub_step = step / 5.0;
    let mut st = first_tick;
    while st <= end_secs + step * 0.1 {
        let sx = frame_to_x(st * sr);
        if sx >= rect.min.x && sx <= rect.max.x {
            ruler_painter.line_segment(
                [Pos2::new(sx, rect.max.y - 3.5), Pos2::new(sx, rect.max.y)],
                Stroke::new(1.0_f32, p.border),
            );
        }
        st += sub_step;
    }

    while t <= end_secs + step * 0.1 {
        let x = frame_to_x(t * sr);
        if x >= rect.min.x - 2.0 && x <= rect.max.x + 2.0 {
            ruler_painter.line_segment(
                [Pos2::new(x, rect.max.y - 7.0), Pos2::new(x, rect.max.y)],
                Stroke::new(1.0_f32, p.text_main),
            );
            ruler_painter.text(
                Pos2::new(x + 3.0, rect.min.y + 1.0),
                Align2::LEFT_TOP,
                format_ruler_time(t),
                FontId::proportional(10.0),
                p.text_main,
            );
        }
        t += step;
    }

    ruler_painter.line_segment(
        [
            Pos2::new(rect.min.x, rect.max.y),
            Pos2::new(rect.max.x, rect.max.y),
        ],
        Stroke::new(1.0_f32, p.border),
    );
}

/// Draws a classic Sound Forge-style vertical Stereo VU Meter.
pub fn draw_vu_meter(ui: &mut Ui, meter: &MeterReading, skin: UiSkin) {
    let p = skin.palette();
    ui.vertical(|ui| {
        ui.label(
            egui::RichText::new("PEAK (dB)")
                .size(10.0)
                .color(p.text_muted),
        );
        let avail_h = (ui.available_height() - 26.0).clamp(110.0, 420.0);
        let (rect, _) = ui.allocate_exact_size(Vec2::new(54.0, avail_h), Sense::hover());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 2.0, Color32::from_rgb(20, 24, 30));
        painter.rect_stroke(rect, 2.0, Stroke::new(1.0_f32, p.border));

        let peaks = [meter.peak_l, meter.peak_r];
        let bar_w = 15.0;
        let pad_top = 8.0;
        let pad_bot = 15.0;
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
                Pos2::new(x0 + bar_w * 0.5, rect.max.y - 3.0),
                Align2::CENTER_BOTTOM,
                if ch == 0 { "L" } else { "R" },
                FontId::monospace(9.5),
                Color32::from_rgb(180, 190, 205),
            );
        }

        let peak_db = amp_to_db(meter.peak_l.max(meter.peak_r));
        let db_text = if peak_db <= -90.0 {
            "-Inf dB".to_string()
        } else {
            format!("{peak_db:.1} dB")
        };
        ui.label(egui::RichText::new(db_text).monospace().size(10.5));
    });
}
