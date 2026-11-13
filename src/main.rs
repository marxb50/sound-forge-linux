mod audio;
mod document;
mod dsp;
mod markers;
mod playback;
mod theme;
mod waveform_ui;

use crate::audio::{export_audio, find_ffmpeg, load_audio, save_wav, AudioBuffer, ExportFormat};
use crate::document::AudioDocument;
use crate::dsp::{
    apply_gain_db, change_speed_range, delete_range, extend_range_with_crossfade, fade_in,
    fade_out, insert_buffer, insert_silence, mix_buffer, normalize_peak, pitch_shift_range,
    reverse_range, silence_range, slice_frames, FadeCurve, SpeedMode,
};
use crate::playback::PlaybackEngine;
use crate::theme::{apply_forge_style, format_timecode, ForgeTheme};
use crate::waveform_ui::{draw_vu_meter, draw_waveform_workspace, WaveformInteraction};
use eframe::egui;
use std::path::PathBuf;

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Sound Forge Linux — Clean-Room Audio Editor")
            .with_inner_size([1340.0, 820.0])
            .with_min_inner_size([920.0, 560.0])
            .with_drag_and_drop(true),
        ..Default::default()
    };

    eframe::run_native(
        "Sound Forge Linux",
        options,
        Box::new(|cc| {
            apply_forge_style(&cc.egui_ctx);
            Ok(Box::new(ForgeApp::new()))
        }),
    )
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ActiveDialog {
    None,
    SpeedAndPitch,
    ExtendSelection,
    GainAndNormalize,
    InsertSilence,
    Export,
    Shortcuts,
    About,
}

struct ForgeApp {
    docs: Vec<AudioDocument>,
    active_tab: usize,
    next_doc_id: usize,
    clipboard: Option<AudioBuffer>,
    playback: PlaybackEngine,
    interaction: WaveformInteraction,

    // Transport & Speed controls
    loop_enabled: bool,
    snap_zero_crossing: bool,
    playback_speed: f64,
    speed_mode: SpeedMode,
    master_volume: f32,

    // Dialog states
    dialog: ActiveDialog,
    dlg_speed_factor: f64,
    dlg_speed_mode: SpeedMode,
    dlg_pitch_semitones: f64,
    dlg_bpm_from: f64,
    dlg_bpm_to: f64,
    dlg_extend_secs: f64,
    dlg_extend_xfade_ms: f64,
    dlg_gain_db: f32,
    dlg_norm_db: f32,
    dlg_fade_curve: FadeCurve,
    dlg_silence_secs: f64,
    dlg_export_format: ExportFormat,
    dlg_export_sr: Option<u32>,

    // Panels & status
    show_markers_panel: bool,
    show_meter_panel: bool,
    ffmpeg_path: Option<PathBuf>,
    status_msg: String,
}

impl ForgeApp {
    fn new() -> Self {
        let ffmpeg_path = find_ffmpeg();
        let status_msg = if ffmpeg_path.is_some() {
            "Pronto — FFmpeg detectado (MP3/FLAC/OGG/M4A + WAV nativo).".to_string()
        } else {
            "Pronto — WAV nativo ativo (instale `ffmpeg` para exportar MP3/FLAC/OGG).".to_string()
        };

        let mut app = Self {
            docs: Vec::new(),
            active_tab: 0,
            next_doc_id: 1,
            clipboard: None,
            playback: PlaybackEngine::new(),
            interaction: WaveformInteraction::default(),
            loop_enabled: false,
            snap_zero_crossing: true,
            playback_speed: 1.0,
            speed_mode: SpeedMode::PreservePitch,
            master_volume: 1.0,
            dialog: ActiveDialog::None,
            dlg_speed_factor: 1.0,
            dlg_speed_mode: SpeedMode::PreservePitch,
            dlg_pitch_semitones: 0.0,
            dlg_bpm_from: 120.0,
            dlg_bpm_to: 120.0,
            dlg_extend_secs: 2.5,
            dlg_extend_xfade_ms: 25.0,
            dlg_gain_db: 0.0,
            dlg_norm_db: -0.5,
            dlg_fade_curve: FadeCurve::SmoothSCurve,
            dlg_silence_secs: 1.0,
            dlg_export_format: ExportFormat::Wav24,
            dlg_export_sr: None,
            show_markers_panel: true,
            show_meter_panel: true,
            ffmpeg_path,
            status_msg,
        };

        // Open file passed via command-line argument if provided (great for Linux .desktop "Open With")
        if let Some(arg_path) = std::env::args().nth(1) {
            let p = PathBuf::from(arg_path);
            if p.exists() {
                app.open_path(p);
            }
        }

        if let Some(err) = &app.playback.init_error {
            app.status_msg = format!("Aviso de áudio: {err}");
        }

        app
    }

    fn active_doc(&self) -> Option<&AudioDocument> {
        self.docs.get(self.active_tab)
    }

    fn active_doc_mut(&mut self) -> Option<&mut AudioDocument> {
        self.docs.get_mut(self.active_tab)
    }

    fn open_file_dialog(&mut self) {
        if let Some(paths) = rfd::FileDialog::new()
            .add_filter(
                "Áudio",
                &["wav", "mp3", "flac", "ogg", "m4a", "aac", "aiff", "opus"],
            )
            .pick_files()
        {
            for p in paths {
                self.open_path(p);
            }
        }
    }

    fn open_path(&mut self, path: PathBuf) {
        match load_audio(&path) {
            Ok(buf) => {
                let title = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("Áudio")
                    .to_string();
                let id = self.next_doc_id;
                self.next_doc_id += 1;
                let doc = AudioDocument::from_buffer(id, title.clone(), Some(path), buf);
                self.playback.set_buffer(doc.buffer.clone());
                self.docs.push(doc);
                self.active_tab = self.docs.len() - 1;
                self.status_msg = format!("Aberto: {title}");
            }
            Err(err) => {
                self.status_msg = format!("Erro ao abrir arquivo: {err}");
            }
        }
    }

    fn new_empty_document(&mut self) {
        let id = self.next_doc_id;
        self.next_doc_id += 1;
        let buf = AudioBuffer::new(44100, 2, vec![0.0; 44100 * 2 * 5]);
        let doc = AudioDocument::from_buffer(id, format!("Sem_Titulo_{id}.wav"), None, buf);
        self.docs.push(doc);
        self.active_tab = self.docs.len() - 1;
        self.status_msg = "Novo arquivo estéreo 44.1 kHz (5s) criado.".to_string();
    }

    fn save_active_wav(&mut self) {
        let Some(doc) = self.active_doc() else {
            return;
        };
        let target_path = if let Some(p) = &doc.path {
            if p.extension().and_then(|e| e.to_str()) == Some("wav") {
                Some(p.clone())
            } else {
                rfd::FileDialog::new()
                    .set_file_name(format!("{}.wav", doc.title))
                    .add_filter("WAV Audio", &["wav"])
                    .save_file()
            }
        } else {
            rfd::FileDialog::new()
                .set_file_name(&doc.title)
                .add_filter("WAV Audio", &["wav"])
                .save_file()
        };

        if let Some(path) = target_path {
            let buf = doc.buffer.clone();
            match save_wav(&buf, &path, ExportFormat::Wav24) {
                Ok(()) => {
                    if let Some(d) = self.active_doc_mut() {
                        d.path = Some(path.clone());
                        d.title = path
                            .file_name()
                            .and_then(|n| n.to_str())
                            .unwrap_or("audio.wav")
                            .to_string();
                        d.dirty = false;
                    }
                    self.status_msg = format!("Salvo: {}", path.display());
                }
                Err(e) => {
                    self.status_msg = format!("Erro ao salvar: {e}");
                }
            }
        }
    }

    fn run_export(&mut self) {
        let Some(doc) = self.active_doc() else {
            return;
        };
        let ext = self.dlg_export_format.extension();
        let stem = PathBuf::from(&doc.title)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("export")
            .to_string();
        let default_name = format!("{stem}_editado.{ext}");

        if let Some(path) = rfd::FileDialog::new()
            .set_file_name(&default_name)
            .add_filter(self.dlg_export_format.label(), &[ext])
            .save_file()
        {
            match export_audio(
                &doc.buffer,
                &path,
                self.dlg_export_format,
                self.dlg_export_sr,
            ) {
                Ok(()) => {
                    self.status_msg = format!("Exportado com sucesso: {}", path.display());
                    self.dialog = ActiveDialog::None;
                }
                Err(e) => {
                    self.status_msg = format!("Erro ao exportar: {e}");
                }
            }
        }
    }

    fn toggle_play(&mut self) {
        if self.playback.is_playing() {
            let cur = self.playback.current_source_frame();
            if let Some(doc) = self.active_doc_mut() {
                doc.cursor_frame = cur;
            }
            self.playback.pause();
            return;
        }
        self.play_active(false);
    }

    fn play_active(&mut self, force_from_cursor: bool) {
        let speed = self.playback_speed;
        let mode = self.speed_mode;
        let loop_on = self.loop_enabled;
        let Some(doc) = self.active_doc() else {
            return;
        };
        let total = doc.buffer.num_frames();
        if total == 0 {
            return;
        }
        let (start, end) = if !force_from_cursor {
            if let Some((s, e)) = doc.selection {
                (s, e)
            } else {
                (doc.cursor_frame.min(total.saturating_sub(1)), total)
            }
        } else {
            (doc.cursor_frame.min(total.saturating_sub(1)), total)
        };
        self.playback
            .play_range(doc.buffer.clone(), start, end, loop_on, speed, mode);
    }

    fn stop_playback(&mut self) {
        self.playback.stop();
    }

    fn action_cut(&mut self) {
        self.stop_playback();
        let Some(doc) = self.active_doc_mut() else {
            return;
        };
        let Some((s, e)) = doc.selection else {
            self.status_msg = "Selecione um trecho antes de Recortar.".to_string();
            return;
        };
        let cut_clip = slice_frames(&doc.buffer, s, e);
        let next_buf = delete_range(&doc.buffer, s, e);
        doc.annotations.adjust_for_edit(s, e, 0);
        doc.commit_buffer(next_buf, s, None);
        self.clipboard = Some(cut_clip);
        self.status_msg = "Trecho recortado para a área de transferência.".to_string();
    }

    fn action_copy(&mut self) {
        let Some(doc) = self.active_doc() else {
            return;
        };
        let Some((s, e)) = doc.selection else {
            self.status_msg = "Selecione um trecho antes de Copiar.".to_string();
            return;
        };
        let copied = slice_frames(&doc.buffer, s, e);
        let secs = doc.buffer.frame_to_secs(e - s);
        self.clipboard = Some(copied);
        self.status_msg = format!("Copiado: {} de áudio.", format_timecode(secs));
    }

    fn action_paste(&mut self) {
        let Some(clip) = self.clipboard.clone() else {
            self.status_msg = "Área de transferência vazia.".to_string();
            return;
        };
        self.stop_playback();
        let Some(doc) = self.active_doc_mut() else {
            return;
        };
        let (ins_at, base_buf) = if let Some((s, e)) = doc.selection {
            doc.annotations.adjust_for_edit(s, e, 0);
            (s, delete_range(&doc.buffer, s, e))
        } else {
            (doc.cursor_frame, (*doc.buffer).clone())
        };
        let next_buf = insert_buffer(&base_buf, ins_at, &clip);
        let added = next_buf.num_frames().saturating_sub(base_buf.num_frames());
        doc.annotations.adjust_for_edit(ins_at, ins_at, added);
        doc.commit_buffer(next_buf, ins_at + added, Some((ins_at, ins_at + added)));
        self.status_msg = "Áudio colado no cursor.".to_string();
    }

    fn action_paste_mix(&mut self) {
        let Some(clip) = self.clipboard.clone() else {
            self.status_msg = "Área de transferência vazia.".to_string();
            return;
        };
        self.stop_playback();
        let Some(doc) = self.active_doc_mut() else {
            return;
        };
        let at = doc.cursor_frame;
        let next_buf = mix_buffer(&doc.buffer, at, &clip, 0.85);
        doc.commit_buffer(next_buf, at, doc.selection);
        self.status_msg = "Mixagem aplicada na posição do cursor.".to_string();
    }

    fn action_delete(&mut self) {
        self.stop_playback();
        let Some(doc) = self.active_doc_mut() else {
            return;
        };
        let Some((s, e)) = doc.selection else {
            return;
        };
        let next_buf = delete_range(&doc.buffer, s, e);
        doc.annotations.adjust_for_edit(s, e, 0);
        doc.commit_buffer(next_buf, s, None);
        self.status_msg = "Trecho removido.".to_string();
    }

    fn action_trim_crop(&mut self) {
        self.stop_playback();
        let Some(doc) = self.active_doc_mut() else {
            return;
        };
        let Some((s, e)) = doc.selection else {
            self.status_msg = "Selecione um trecho para Aparar (Trim/Crop).".to_string();
            return;
        };
        let next_buf = slice_frames(&doc.buffer, s, e);
        let new_len = next_buf.num_frames();
        // Shift markers into cropped window
        doc.annotations.markers.retain(|m| m.frame >= s && m.frame <= e);
        for m in &mut doc.annotations.markers {
            m.frame -= s;
        }
        doc.annotations.regions.retain(|r| r.end_frame > s && r.start_frame < e);
        for r in &mut doc.annotations.regions {
            r.start_frame = r.start_frame.saturating_sub(s);
            r.end_frame = r.end_frame.saturating_sub(s).min(new_len);
        }
        doc.commit_buffer(next_buf, 0, None);
        doc.zoom_fit();
        self.status_msg = "Arquivo aparado para a seleção (Trim/Crop).".to_string();
    }

    fn apply_speed_to_doc(&mut self, speed_factor: f64, mode: SpeedMode) {
        self.stop_playback();
        let Some(doc) = self.active_doc_mut() else {
            return;
        };
        let (s, e) = doc.target_range();
        let old_len = e - s;
        let next_buf = change_speed_range(&doc.buffer, s, e, speed_factor, mode);
        let new_len = next_buf.num_frames() - (doc.buffer.num_frames() - old_len);
        doc.annotations.adjust_for_edit(s, e, new_len);
        let new_sel = doc.selection.map(|_| (s, s + new_len));
        doc.commit_buffer(next_buf, s, new_sel);
        let pct = (speed_factor - 1.0) * 100.0;
        let mode_lbl = match mode {
            SpeedMode::PreservePitch => "mantendo o tom (WSOLA)",
            SpeedMode::Varispeed => "Varispeed",
        };
        self.status_msg = format!(
            "Velocidade aplicada: {speed_factor:.2}x ({pct:+.1}%) {mode_lbl}."
        );
    }

    fn apply_pitch_to_doc(&mut self, semitones: f64) {
        self.stop_playback();
        let Some(doc) = self.active_doc_mut() else {
            return;
        };
        let (s, e) = doc.target_range();
        let old_len = e - s;
        let next_buf = pitch_shift_range(&doc.buffer, s, e, semitones);
        let new_len = next_buf.num_frames() - (doc.buffer.num_frames() - old_len);
        doc.annotations.adjust_for_edit(s, e, new_len);
        let new_sel = doc.selection.map(|_| (s, s + new_len));
        doc.commit_buffer(next_buf, s, new_sel);
        self.status_msg = format!("Tom alterado em {semitones:+.1} semitons.");
    }

    fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        if ctx.wants_keyboard_input() {
            return;
        }
        let ctrl = ctx.input(|i| i.modifiers.ctrl);
        let shift = ctx.input(|i| i.modifiers.shift);

        if ctx.input(|i| i.key_pressed(egui::Key::Space)) {
            self.toggle_play();
        }
        if ctx.input(|i| i.key_pressed(egui::Key::Enter)) {
            self.stop_playback();
        }
        if ctx.input(|i| i.key_pressed(egui::Key::M)) {
            if let Some(doc) = self.active_doc_mut() {
                doc.annotations.add_marker(doc.cursor_frame, None);
                self.status_msg = "Marcador inserido no cursor.".to_string();
            }
        }
        if ctx.input(|i| i.key_pressed(egui::Key::R)) {
            if let Some(doc) = self.active_doc_mut() {
                if let Some((s, e)) = doc.selection {
                    doc.annotations.add_region(s, e, None);
                    self.status_msg = "Região criada a partir da seleção.".to_string();
                }
            }
        }
        if ctx.input(|i| i.key_pressed(egui::Key::L)) {
            self.loop_enabled = !self.loop_enabled;
            self.playback.set_loop(self.loop_enabled);
        }
        if ctx.input(|i| i.key_pressed(egui::Key::Z)) && !ctrl {
            self.snap_zero_crossing = !self.snap_zero_crossing;
        }
        if ctx.input(|i| i.key_pressed(egui::Key::Home)) {
            if let Some(doc) = self.active_doc_mut() {
                doc.cursor_frame = 0;
            }
        }
        if ctx.input(|i| i.key_pressed(egui::Key::End)) {
            if let Some(doc) = self.active_doc_mut() {
                doc.cursor_frame = doc.buffer.num_frames();
            }
        }
        if ctx.input(|i| i.key_pressed(egui::Key::Delete)) {
            self.action_delete();
        }

        if ctrl {
            if ctx.input(|i| i.key_pressed(egui::Key::O)) {
                self.open_file_dialog();
            }
            if ctx.input(|i| i.key_pressed(egui::Key::S)) {
                self.save_active_wav();
            }
            if ctx.input(|i| i.key_pressed(egui::Key::Z)) {
                if shift {
                    if let Some(doc) = self.active_doc_mut() {
                        doc.redo();
                    }
                } else if let Some(doc) = self.active_doc_mut() {
                    doc.undo();
                }
            }
            if ctx.input(|i| i.key_pressed(egui::Key::Y)) {
                if let Some(doc) = self.active_doc_mut() {
                    doc.redo();
                }
            }
            if ctx.input(|i| i.key_pressed(egui::Key::X)) {
                self.action_cut();
            }
            if ctx.input(|i| i.key_pressed(egui::Key::C)) {
                self.action_copy();
            }
            if ctx.input(|i| i.key_pressed(egui::Key::V)) {
                self.action_paste();
            }
            if ctx.input(|i| i.key_pressed(egui::Key::T)) {
                self.action_trim_crop();
            }
            if ctx.input(|i| i.key_pressed(egui::Key::A)) {
                if let Some(doc) = self.active_doc_mut() {
                    let n = doc.buffer.num_frames();
                    if n > 0 {
                        doc.selection = Some((0, n));
                    }
                }
            }
            if ctx.input(|i| i.key_pressed(egui::Key::Num0)) {
                if let Some(doc) = self.active_doc_mut() {
                    doc.zoom_fit();
                }
            }
        }
    }

    fn handle_dropped_files(&mut self, ctx: &egui::Context) {
        let dropped: Vec<PathBuf> = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .filter_map(|f| f.path.clone())
                .collect()
        });
        for p in dropped {
            self.open_path(p);
        }
    }
}

impl eframe::App for ForgeApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.handle_dropped_files(ctx);
        self.handle_shortcuts(ctx);

        if self.playback.is_playing() {
            ctx.request_repaint_after(std::time::Duration::from_millis(16));
        }

        // Top Menu Bar
        egui::TopBottomPanel::top("menu_bar").show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                ui.menu_button("Arquivo", |ui| {
                    if ui.button("Novo (Estéreo 44.1 kHz)").clicked() {
                        self.new_empty_document();
                        ui.close_menu();
                    }
                    if ui.button("Abrir Áudio... (Ctrl+O)").clicked() {
                        self.open_file_dialog();
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui.button("Salvar WAV (Ctrl+S)").clicked() {
                        self.save_active_wav();
                        ui.close_menu();
                    }
                    if ui.button("Exportar (MP3 / FLAC / OGG / WAV)...").clicked() {
                        self.dialog = ActiveDialog::Export;
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui.button("Fechar Aba Atual").clicked() {
                        if !self.docs.is_empty() {
                            self.stop_playback();
                            self.docs.remove(self.active_tab);
                            self.active_tab = self.active_tab.min(self.docs.len().saturating_sub(1));
                        }
                        ui.close_menu();
                    }
                    if ui.button("Sair").clicked() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                });

                ui.menu_button("Editar", |ui| {
                    if ui.button("Desfazer (Ctrl+Z)").clicked() {
                        if let Some(doc) = self.active_doc_mut() {
                            doc.undo();
                        }
                        ui.close_menu();
                    }
                    if ui.button("Refazer (Ctrl+Y)").clicked() {
                        if let Some(doc) = self.active_doc_mut() {
                            doc.redo();
                        }
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui.button("Recortar (Ctrl+X)").clicked() {
                        self.action_cut();
                        ui.close_menu();
                    }
                    if ui.button("Copiar (Ctrl+C)").clicked() {
                        self.action_copy();
                        ui.close_menu();
                    }
                    if ui.button("Colar Inserindo (Ctrl+V)").clicked() {
                        self.action_paste();
                        ui.close_menu();
                    }
                    if ui.button("Colar Mixando").clicked() {
                        self.action_paste_mix();
                        ui.close_menu();
                    }
                    if ui.button("Excluir Seleção (Del)").clicked() {
                        self.action_delete();
                        ui.close_menu();
                    }
                    if ui.button("Aparar para Seleção / Trim (Ctrl+T)").clicked() {
                        self.action_trim_crop();
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui.button("Selecionar Tudo (Ctrl+A)").clicked() {
                        if let Some(doc) = self.active_doc_mut() {
                            let n = doc.buffer.num_frames();
                            if n > 0 {
                                doc.selection = Some((0, n));
                            }
                        }
                        ui.close_menu();
                    }
                    if ui.button("Limpar Seleção").clicked() {
                        if let Some(doc) = self.active_doc_mut() {
                            doc.selection = None;
                        }
                        ui.close_menu();
                    }
                });

                ui.menu_button("Exibir", |ui| {
                    if ui.button("Zoom In").clicked() {
                        if let Some(doc) = self.active_doc_mut() {
                            doc.zoom_at(0.5, 0.5);
                        }
                        ui.close_menu();
                    }
                    if ui.button("Zoom Out").clicked() {
                        if let Some(doc) = self.active_doc_mut() {
                            doc.zoom_at(2.0, 0.5);
                        }
                        ui.close_menu();
                    }
                    if ui.button("Zoom na Seleção").clicked() {
                        if let Some(doc) = self.active_doc_mut() {
                            doc.zoom_selection();
                        }
                        ui.close_menu();
                    }
                    if ui.button("Mostrar Tudo (Ctrl+0)").clicked() {
                        if let Some(doc) = self.active_doc_mut() {
                            doc.zoom_fit();
                        }
                        ui.close_menu();
                    }
                    ui.separator();
                    ui.checkbox(&mut self.show_markers_panel, "Painel de Marcadores");
                    ui.checkbox(&mut self.show_meter_panel, "Medidor VU Estéreo");
                    ui.checkbox(&mut self.snap_zero_crossing, "Snap Zero-Crossing (Z)");
                });

                ui.menu_button("Processar", |ui| {
                    if ui.button("Acelerar / Ralentar & Pitch (Time Stretch)...").clicked() {
                        self.dlg_speed_factor = self.playback_speed;
                        self.dlg_speed_mode = self.speed_mode;
                        self.dialog = ActiveDialog::SpeedAndPitch;
                        ui.close_menu();
                    }
                    if ui.button("Estender Trecho com Crossfade...").clicked() {
                        self.dialog = ActiveDialog::ExtendSelection;
                        ui.close_menu();
                    }
                    if ui.button("Ganho / Normalizar...").clicked() {
                        self.dialog = ActiveDialog::GainAndNormalize;
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui.button("Normalizar Pico (-0.5 dB)").clicked() {
                        self.stop_playback();
                        if let Some(doc) = self.active_doc_mut() {
                            let (s, e) = doc.target_range();
                            let next = normalize_peak(&doc.buffer, s, e, -0.5);
                            doc.commit_buffer(next, doc.cursor_frame, doc.selection);
                        }
                        self.status_msg = "Normalizado para -0.5 dB.".to_string();
                        ui.close_menu();
                    }
                    if ui.button("Fade In (Suave)").clicked() {
                        self.stop_playback();
                        if let Some(doc) = self.active_doc_mut() {
                            let (s, e) = doc.target_range();
                            let next = fade_in(&doc.buffer, s, e, FadeCurve::SmoothSCurve);
                            doc.commit_buffer(next, doc.cursor_frame, doc.selection);
                        }
                        self.status_msg = "Fade In aplicado.".to_string();
                        ui.close_menu();
                    }
                    if ui.button("Fade Out (Suave)").clicked() {
                        self.stop_playback();
                        if let Some(doc) = self.active_doc_mut() {
                            let (s, e) = doc.target_range();
                            let next = fade_out(&doc.buffer, s, e, FadeCurve::SmoothSCurve);
                            doc.commit_buffer(next, doc.cursor_frame, doc.selection);
                        }
                        self.status_msg = "Fade Out aplicado.".to_string();
                        ui.close_menu();
                    }
                    if ui.button("Inverter Áudio (Reverse)").clicked() {
                        self.stop_playback();
                        if let Some(doc) = self.active_doc_mut() {
                            let (s, e) = doc.target_range();
                            let next = reverse_range(&doc.buffer, s, e);
                            doc.commit_buffer(next, doc.cursor_frame, doc.selection);
                        }
                        self.status_msg = "Áudio invertido.".to_string();
                        ui.close_menu();
                    }
                    if ui.button("Silenciar Seleção (Mute)").clicked() {
                        self.stop_playback();
                        if let Some(doc) = self.active_doc_mut() {
                            let (s, e) = doc.target_range();
                            let next = silence_range(&doc.buffer, s, e);
                            doc.commit_buffer(next, doc.cursor_frame, doc.selection);
                        }
                        self.status_msg = "Trecho silenciado.".to_string();
                        ui.close_menu();
                    }
                });

                ui.menu_button("Inserir", |ui| {
                    if ui.button("Marcador no Cursor (M)").clicked() {
                        if let Some(doc) = self.active_doc_mut() {
                            doc.annotations.add_marker(doc.cursor_frame, None);
                        }
                        ui.close_menu();
                    }
                    if ui.button("Região da Seleção (R)").clicked() {
                        if let Some(doc) = self.active_doc_mut() {
                            if let Some((s, e)) = doc.selection {
                                doc.annotations.add_region(s, e, None);
                            }
                        }
                        ui.close_menu();
                    }
                    if ui.button("Silêncio no Cursor...").clicked() {
                        self.dialog = ActiveDialog::InsertSilence;
                        ui.close_menu();
                    }
                });

                ui.menu_button("Ajuda", |ui| {
                    if ui.button("Atalhos de Teclado").clicked() {
                        self.dialog = ActiveDialog::Shortcuts;
                        ui.close_menu();
                    }
                    if ui.button("Sobre o Sound Forge Linux").clicked() {
                        self.dialog = ActiveDialog::About;
                        ui.close_menu();
                    }
                });
            });
        });

        // Main Action Toolbar
        egui::TopBottomPanel::top("main_toolbar").show(ctx, |ui| {
            ui.horizontal_wrapped(|ui| {
                if ui.button("📂 Abrir").clicked() {
                    self.open_file_dialog();
                }
                if ui.button("💾 Salvar WAV").clicked() {
                    self.save_active_wav();
                }
                if ui.button("📤 Exportar").clicked() {
                    self.dialog = ActiveDialog::Export;
                }
                ui.separator();
                if ui.button("↩ Desfazer").clicked() {
                    if let Some(doc) = self.active_doc_mut() {
                        doc.undo();
                    }
                }
                if ui.button("↪ Refazer").clicked() {
                    if let Some(doc) = self.active_doc_mut() {
                        doc.redo();
                    }
                }
                ui.separator();
                if ui.button("✂ Recortar").clicked() {
                    self.action_cut();
                }
                if ui.button("📋 Copiar").clicked() {
                    self.action_copy();
                }
                if ui.button("📌 Colar").clicked() {
                    self.action_paste();
                }
                if ui.button("🔪 Aparar (Trim)").clicked() {
                    self.action_trim_crop();
                }
                ui.separator();
                if ui.button("🔍+").on_hover_text("Zoom In").clicked() {
                    if let Some(doc) = self.active_doc_mut() {
                        doc.zoom_at(0.5, 0.5);
                    }
                }
                if ui.button("🔍-").on_hover_text("Zoom Out").clicked() {
                    if let Some(doc) = self.active_doc_mut() {
                        doc.zoom_at(2.0, 0.5);
                    }
                }
                if ui.button("⛶ Seleção").clicked() {
                    if let Some(doc) = self.active_doc_mut() {
                        doc.zoom_selection();
                    }
                }
                if ui.button("↔ Tudo").clicked() {
                    if let Some(doc) = self.active_doc_mut() {
                        doc.zoom_fit();
                    }
                }
                ui.separator();
                if ui.button("🚩 +Marcador (M)").clicked() {
                    if let Some(doc) = self.active_doc_mut() {
                        doc.annotations.add_marker(doc.cursor_frame, None);
                    }
                }
                if ui.button("⏩ Velocidade / Tom").clicked() {
                    self.dlg_speed_factor = self.playback_speed;
                    self.dlg_speed_mode = self.speed_mode;
                    self.dialog = ActiveDialog::SpeedAndPitch;
                }
                if ui.button("🔁 Estender Trecho").clicked() {
                    self.dialog = ActiveDialog::ExtendSelection;
                }
                ui.separator();
                ui.toggle_value(&mut self.snap_zero_crossing, "⚡ Zero-Cross (Z)")
                    .on_hover_text("Ajustar seleção automaticamente para cruzamento por zero (evita estalos)");
            });
        });

        // Transport & Live Speed Bar
        egui::TopBottomPanel::top("transport_bar").show(ctx, |ui| {
            ui.horizontal_wrapped(|ui| {
                let playing = self.playback.is_playing();
                let play_lbl = if playing { "⏸ Pausar" } else { "▶ Tocar" };
                if ui
                    .add(
                        egui::Button::new(
                            egui::RichText::new(play_lbl)
                                .strong()
                                .color(if playing {
                                    ForgeTheme::ACCENT_AMBER
                                } else {
                                    ForgeTheme::ACCENT_GREEN
                                }),
                        ),
                    )
                    .clicked()
                {
                    self.toggle_play();
                }
                if ui.button("⏹ Parar").clicked() {
                    self.stop_playback();
                }
                if ui.button("⏮ Início").clicked() {
                    if let Some(doc) = self.active_doc_mut() {
                        doc.cursor_frame = 0;
                    }
                }
                if ui.toggle_value(&mut self.loop_enabled, "🔁 Loop (L)").clicked() {
                    self.playback.set_loop(self.loop_enabled);
                }

                ui.separator();
                ui.label(egui::RichText::new("Velocidade:").strong());
                let speed_changed = ui
                    .add(
                        egui::Slider::new(&mut self.playback_speed, 0.50..=2.00)
                            .fixed_decimals(2)
                            .suffix("x"),
                    )
                    .changed();

                for &(preset, lbl) in &[
                    (0.75, "0.75x"),
                    (0.85, "0.85x"),
                    (1.00, "1.0x"),
                    (1.15, "1.15x"),
                    (1.25, "1.25x"),
                ] {
                    if ui
                        .selectable_label((self.playback_speed - preset).abs() < 0.005, lbl)
                        .clicked()
                    {
                        self.playback_speed = preset;
                        if playing {
                            self.play_active(false);
                        }
                    }
                }

                let mut preserve_pitch = self.speed_mode == SpeedMode::PreservePitch;
                if ui
                    .checkbox(&mut preserve_pitch, "Preservar Tom")
                    .on_hover_text("Acelera/ralenta sem afinar ou engrossar a música (WSOLA)")
                    .changed()
                {
                    self.speed_mode = if preserve_pitch {
                        SpeedMode::PreservePitch
                    } else {
                        SpeedMode::Varispeed
                    };
                    if playing {
                        self.play_active(false);
                    }
                }

                if speed_changed && playing && self.speed_mode == SpeedMode::Varispeed {
                    self.play_active(false);
                }

                if (self.playback_speed - 1.0).abs() > 0.005 {
                    if ui
                        .button("✔ Aplicar ao Áudio")
                        .on_hover_text("Grava a velocidade atual permanentemente no trecho/arquivo")
                        .clicked()
                    {
                        self.apply_speed_to_doc(self.playback_speed, self.speed_mode);
                        self.playback_speed = 1.0;
                    }
                }

                ui.separator();
                ui.label("Vol:");
                if ui
                    .add(egui::Slider::new(&mut self.master_volume, 0.0..=1.5).show_value(false))
                    .changed()
                {
                    self.playback.set_master_gain(self.master_volume);
                }
            });
        });

        // Bottom Status Bar
        egui::TopBottomPanel::bottom("status_bar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                if let Some(doc) = self.active_doc() {
                    let sr = doc.buffer.sample_rate;
                    let ch_str = if doc.buffer.channels == 1 {
                        "Mono"
                    } else {
                        "Estéreo"
                    };
                    let cur_s = doc.buffer.frame_to_secs(doc.cursor_frame);
                    ui.label(
                        egui::RichText::new(format!("Cursor: {}", format_timecode(cur_s)))
                            .monospace(),
                    );
                    ui.separator();
                    if let Some((s, e)) = doc.selection {
                        let ss = doc.buffer.frame_to_secs(s);
                        let es = doc.buffer.frame_to_secs(e);
                        let ls = doc.buffer.frame_to_secs(e - s);
                        ui.label(
                            egui::RichText::new(format!(
                                "Sel: {} → {} (Dur: {})",
                                format_timecode(ss),
                                format_timecode(es),
                                format_timecode(ls)
                            ))
                            .monospace()
                            .color(ForgeTheme::SELECTION_BORDER),
                        );
                    } else {
                        ui.label(
                            egui::RichText::new(format!(
                                "Duração: {}",
                                format_timecode(doc.buffer.duration_secs())
                            ))
                            .monospace(),
                        );
                    }
                    ui.separator();
                    ui.label(
                        egui::RichText::new(format!("{sr} Hz • {ch_str} • 32-bit float"))
                            .monospace()
                            .color(ForgeTheme::TEXT_MUTED),
                    );
                    ui.separator();
                }
                ui.label(
                    egui::RichText::new(&self.status_msg).color(ForgeTheme::TEXT_MUTED),
                );
            });
        });

        // Left Dock: Markers & Regions
        if self.show_markers_panel && !self.docs.is_empty() {
            egui::SidePanel::left("markers_panel")
                .default_width(235.0)
                .show(ctx, |ui| {
                    ui.heading(egui::RichText::new("Marcadores & Regiões").size(14.0));
                    ui.separator();
                    if let Some(doc) = self.active_doc_mut() {
                        ui.horizontal(|ui| {
                            if ui.button("+ Marcador (M)").clicked() {
                                doc.annotations.add_marker(doc.cursor_frame, None);
                            }
                            if ui.button("+ Região (R)").clicked() {
                                if let Some((s, e)) = doc.selection {
                                    doc.annotations.add_region(s, e, None);
                                }
                            }
                        });
                        ui.add_space(6.0);
                        egui::ScrollArea::vertical().show(ui, |ui| {
                            ui.label(
                                egui::RichText::new("Marcadores")
                                    .strong()
                                    .color(ForgeTheme::MARKER),
                            );
                            if doc.annotations.markers.is_empty() {
                                ui.label(
                                    egui::RichText::new("Nenhum marcador (tecle M).")
                                        .size(11.5)
                                        .color(ForgeTheme::TEXT_MUTED),
                                );
                            }
                            let mut remove_m = None;
                            let mut jump_to = None;
                            let sr = doc.buffer.sample_rate;
                            for m in &mut doc.annotations.markers {
                                ui.horizontal(|ui| {
                                    let t = m.frame as f64 / sr as f64;
                                    if ui
                                        .button(format!("▶ {}", format_timecode(t)))
                                        .clicked()
                                    {
                                        jump_to = Some(m.frame);
                                    }
                                    ui.add(
                                        egui::TextEdit::singleline(&mut m.name)
                                            .desired_width(92.0),
                                    );
                                    if ui.button("✕").clicked() {
                                        remove_m = Some(m.id);
                                    }
                                });
                            }
                            if let Some(f) = jump_to {
                                doc.cursor_frame = f;
                            }
                            if let Some(id) = remove_m {
                                doc.annotations.remove_marker(id);
                            }

                            ui.add_space(10.0);
                            ui.label(
                                egui::RichText::new("Regiões")
                                    .strong()
                                    .color(ForgeTheme::REGION_BORDER),
                            );
                            let mut remove_r = None;
                            let mut select_r = None;
                            for r in &mut doc.annotations.regions {
                                ui.horizontal(|ui| {
                                    if ui.button("🎯").on_hover_text("Selecionar região").clicked() {
                                        select_r = Some((r.start_frame, r.end_frame));
                                    }
                                    ui.add(
                                        egui::TextEdit::singleline(&mut r.name)
                                            .desired_width(135.0),
                                    );
                                    if ui.button("✕").clicked() {
                                        remove_r = Some(r.id);
                                    }
                                });
                            }
                            if let Some((s, e)) = select_r {
                                doc.selection = Some((s, e));
                                doc.cursor_frame = s;
                            }
                            if let Some(id) = remove_r {
                                doc.annotations.remove_region(id);
                            }
                        });
                    }
                });
        }

        // Right Dock: Stereo VU Meter
        if self.show_meter_panel {
            egui::SidePanel::right("meter_panel")
                .exact_width(78.0)
                .show(ctx, |ui| {
                    let meter = self.playback.meter_reading();
                    draw_vu_meter(ui, &meter);
                });
        }

        // Central Workspace: Tabs + Waveform Canvas
        egui::CentralPanel::default().show(ctx, |ui| {
            if self.docs.is_empty() {
                ui.centered_and_justified(|ui| {
                    ui.vertical_centered(|ui| {
                        ui.add_space(80.0);
                        ui.heading("Sound Forge Linux (Clean-Room Edition)");
                        ui.add_space(8.0);
                        ui.label(
                            egui::RichText::new(
                                "Editor de áudio nativo em Rust inspirado no Sound Forge Pro 10.0",
                            )
                            .color(ForgeTheme::TEXT_MUTED),
                        );
                        ui.add_space(18.0);
                        ui.horizontal(|ui| {
                            if ui.button("📂 Abrir Música / Áudio (MP3, WAV, FLAC, OGG)").clicked()
                            {
                                self.open_file_dialog();
                            }
                            if ui.button("📄 Novo Projeto Vazio").clicked() {
                                self.new_empty_document();
                            }
                        });
                        ui.add_space(12.0);
                        ui.label(
                            egui::RichText::new("Dica: você também pode arrastar e soltar arquivos de áudio aqui.")
                                .color(ForgeTheme::TEXT_MUTED),
                        );
                    });
                });
                return;
            }

            // Document Tab Bar
            let mut close_idx = None;
            let mut switched_tab = false;
            ui.horizontal_wrapped(|ui| {
                for (idx, doc) in self.docs.iter().enumerate() {
                    let label = if doc.dirty {
                        format!("{} *", doc.title)
                    } else {
                        doc.title.clone()
                    };
                    let selected = idx == self.active_tab;
                    if ui.selectable_label(selected, label).clicked() && !selected {
                        self.active_tab = idx;
                        switched_tab = true;
                    }
                    if ui.small_button("✕").clicked() {
                        close_idx = Some(idx);
                    }
                    ui.separator();
                }
            });
            if switched_tab {
                self.stop_playback();
                if let Some(doc) = self.active_doc() {
                    self.playback.set_buffer(doc.buffer.clone());
                }
            }
            if let Some(idx) = close_idx {
                self.stop_playback();
                self.docs.remove(idx);
                self.active_tab = self.active_tab.min(self.docs.len().saturating_sub(1));
                return;
            }

            ui.separator();

            let playhead = if self.playback.is_playing() {
                Some(self.playback.current_source_frame())
            } else {
                None
            };

            let snap = self.snap_zero_crossing;
            let mut play_req = None;
            if let Some(doc) = self.docs.get_mut(self.active_tab) {
                play_req =
                    draw_waveform_workspace(ui, doc, &mut self.interaction, playhead, snap);
            }
            if let Some(frame) = play_req {
                if let Some(doc) = self.active_doc_mut() {
                    doc.cursor_frame = frame;
                }
                self.play_active(true);
            }
        });

        self.draw_dialogs(ctx);
    }
}

impl ForgeApp {
    fn draw_dialogs(&mut self, ctx: &egui::Context) {
        match self.dialog {
            ActiveDialog::None => {}
            ActiveDialog::SpeedAndPitch => {
                let mut open = true;
                egui::Window::new("Acelerar / Ralentar & Pitch (Time Stretch)")
                    .open(&mut open)
                    .resizable(false)
                    .show(ctx, |ui| {
                        ui.label("Ajuste a velocidade da música ou do trecho selecionado:");
                        ui.add_space(6.0);

                        ui.horizontal(|ui| {
                            ui.label("Fator de velocidade:");
                            ui.add(
                                egui::Slider::new(&mut self.dlg_speed_factor, 0.25..=2.50)
                                    .fixed_decimals(3)
                                    .suffix("x"),
                            );
                        });

                        let pct = (self.dlg_speed_factor - 1.0) * 100.0;
                        ui.label(
                            egui::RichText::new(format!(
                                "Variação: {pct:+.1}% ({})",
                                if pct > 0.5 {
                                    "Mais rápido"
                                } else if pct < -0.5 {
                                    "Mais lento"
                                } else {
                                    "Original"
                                }
                            ))
                            .color(ForgeTheme::ACCENT_AMBER),
                        );

                        ui.add_space(4.0);
                        ui.horizontal(|ui| {
                            for &(val, lbl) in &[
                                (0.75, "75%"),
                                (0.85, "85%"),
                                (0.90, "90%"),
                                (0.95, "95%"),
                                (1.05, "105%"),
                                (1.10, "110%"),
                                (1.25, "125%"),
                            ] {
                                if ui.button(lbl).clicked() {
                                    self.dlg_speed_factor = val;
                                }
                            }
                        });

                        ui.separator();
                        ui.label("Calculadora por BPM (ex.: música a 120 BPM para 108 BPM):");
                        ui.horizontal(|ui| {
                            ui.label("De:");
                            ui.add(egui::DragValue::new(&mut self.dlg_bpm_from).range(20.0..=300.0));
                            ui.label("Para:");
                            ui.add(egui::DragValue::new(&mut self.dlg_bpm_to).range(20.0..=300.0));
                            if ui.button("Calcular Fator").clicked() && self.dlg_bpm_from > 0.0 {
                                self.dlg_speed_factor =
                                    (self.dlg_bpm_to / self.dlg_bpm_from).clamp(0.25, 2.50);
                            }
                        });

                        ui.separator();
                        ui.radio_value(
                            &mut self.dlg_speed_mode,
                            SpeedMode::PreservePitch,
                            "Preservar Tom Original (WSOLA Time-Stretch — ideal para música/dança)",
                        );
                        ui.radio_value(
                            &mut self.dlg_speed_mode,
                            SpeedMode::Varispeed,
                            "Varispeed (muda velocidade e tom juntos, estilo fita/vinil)",
                        );

                        ui.add_space(6.0);
                        ui.horizontal(|ui| {
                            if ui.button("✔ Aplicar Velocidade").clicked() {
                                let factor = self.dlg_speed_factor;
                                let mode = self.dlg_speed_mode;
                                self.apply_speed_to_doc(factor, mode);
                                self.dialog = ActiveDialog::None;
                            }
                            if ui.button("▶ Testar no Player").clicked() {
                                self.playback_speed = self.dlg_speed_factor;
                                self.speed_mode = self.dlg_speed_mode;
                                self.play_active(false);
                            }
                        });

                        ui.separator();
                        ui.label("Mudar Apenas o Tom (Pitch Shift sem alterar duração):");
                        ui.horizontal(|ui| {
                            ui.add(
                                egui::Slider::new(&mut self.dlg_pitch_semitones, -12.0..=12.0)
                                    .step_by(0.5)
                                    .suffix(" semitons"),
                            );
                            if ui.button("Aplicar Pitch").clicked() {
                                let st = self.dlg_pitch_semitones;
                                self.apply_pitch_to_doc(st);
                                self.dialog = ActiveDialog::None;
                            }
                        });
                    });
                if !open {
                    self.dialog = ActiveDialog::None;
                }
            }

            ActiveDialog::ExtendSelection => {
                let mut open = true;
                egui::Window::new("Estender Trecho com Crossfade")
                    .open(&mut open)
                    .resizable(false)
                    .show(ctx, |ui| {
                        ui.label("Repete o trecho selecionado com crossfade suave para alongar frases musicais:");
                        ui.horizontal(|ui| {
                            ui.label("Segundos extras:");
                            ui.add(
                                egui::DragValue::new(&mut self.dlg_extend_secs)
                                    .speed(0.1)
                                    .range(0.1..=60.0)
                                    .suffix(" s"),
                            );
                            for &preset in &[1.5, 2.5, 3.5] {
                                if ui.button(format!("+{preset}s")).clicked() {
                                    self.dlg_extend_secs = preset;
                                }
                            }
                        });
                        ui.horizontal(|ui| {
                            ui.label("Crossfade:");
                            ui.add(
                                egui::DragValue::new(&mut self.dlg_extend_xfade_ms)
                                    .speed(1.0)
                                    .range(5.0..=250.0)
                                    .suffix(" ms"),
                            );
                        });
                        ui.add_space(6.0);
                        ui.horizontal(|ui| {
                            if ui.button("✔ Aplicar Extensão").clicked() {
                                self.stop_playback();
                                let secs = self.dlg_extend_secs;
                                let xfade = self.dlg_extend_xfade_ms;
                                if let Some(doc) = self.active_doc_mut() {
                                    let (s, e) = doc.target_range();
                                    let old_len = doc.buffer.num_frames();
                                    let next = extend_range_with_crossfade(&doc.buffer, s, e, secs, xfade);
                                    let added = next.num_frames().saturating_sub(old_len);
                                    doc.annotations.adjust_for_edit(e, e, added);
                                    doc.commit_buffer(next, s, Some((s, e + added)));
                                }
                                self.status_msg = format!("Trecho estendido em +{secs:.2}s.");
                                self.dialog = ActiveDialog::None;
                            }
                            if ui.button("Cancelar").clicked() {
                                self.dialog = ActiveDialog::None;
                            }
                        });
                    });
                if !open {
                    self.dialog = ActiveDialog::None;
                }
            }

            ActiveDialog::GainAndNormalize => {
                let mut open = true;
                egui::Window::new("Ganho, Normalização e Fades")
                    .open(&mut open)
                    .resizable(false)
                    .show(ctx, |ui| {
                        ui.horizontal(|ui| {
                            ui.label("Ganho (dB):");
                            ui.add(egui::Slider::new(&mut self.dlg_gain_db, -24.0..=24.0).suffix(" dB"));
                            if ui.button("Aplicar Ganho").clicked() {
                                self.stop_playback();
                                let db = self.dlg_gain_db;
                                if let Some(doc) = self.active_doc_mut() {
                                    let (s, e) = doc.target_range();
                                    let next = apply_gain_db(&doc.buffer, s, e, db);
                                    doc.commit_buffer(next, doc.cursor_frame, doc.selection);
                                }
                                self.status_msg = format!("Ganho de {db:+.1} dB aplicado.");
                            }
                        });

                        ui.separator();
                        ui.horizontal(|ui| {
                            ui.label("Normalizar pico para:");
                            ui.add(egui::Slider::new(&mut self.dlg_norm_db, -12.0..=0.0).suffix(" dB"));
                            if ui.button("Normalizar").clicked() {
                                self.stop_playback();
                                let db = self.dlg_norm_db;
                                if let Some(doc) = self.active_doc_mut() {
                                    let (s, e) = doc.target_range();
                                    let next = normalize_peak(&doc.buffer, s, e, db);
                                    doc.commit_buffer(next, doc.cursor_frame, doc.selection);
                                }
                                self.status_msg = format!("Normalizado para {db:.1} dB.");
                            }
                        });

                        ui.separator();
                        ui.label("Curva de Fade:");
                        ui.horizontal(|ui| {
                            for curve in [
                                FadeCurve::Linear,
                                FadeCurve::SmoothSCurve,
                                FadeCurve::Exponential,
                            ] {
                                ui.radio_value(&mut self.dlg_fade_curve, curve, curve.label());
                            }
                        });
                        ui.horizontal(|ui| {
                            if ui.button("Aplicar Fade In").clicked() {
                                self.stop_playback();
                                let curve = self.dlg_fade_curve;
                                if let Some(doc) = self.active_doc_mut() {
                                    let (s, e) = doc.target_range();
                                    let next = fade_in(&doc.buffer, s, e, curve);
                                    doc.commit_buffer(next, doc.cursor_frame, doc.selection);
                                }
                            }
                            if ui.button("Aplicar Fade Out").clicked() {
                                self.stop_playback();
                                let curve = self.dlg_fade_curve;
                                if let Some(doc) = self.active_doc_mut() {
                                    let (s, e) = doc.target_range();
                                    let next = fade_out(&doc.buffer, s, e, curve);
                                    doc.commit_buffer(next, doc.cursor_frame, doc.selection);
                                }
                            }
                        });
                    });
                if !open {
                    self.dialog = ActiveDialog::None;
                }
            }

            ActiveDialog::InsertSilence => {
                let mut open = true;
                egui::Window::new("Inserir Silêncio")
                    .open(&mut open)
                    .resizable(false)
                    .show(ctx, |ui| {
                        ui.horizontal(|ui| {
                            ui.label("Duração (segundos):");
                            ui.add(
                                egui::DragValue::new(&mut self.dlg_silence_secs)
                                    .speed(0.1)
                                    .range(0.05..=120.0)
                                    .suffix(" s"),
                            );
                        });
                        if ui.button("✔ Inserir no Cursor").clicked() {
                            self.stop_playback();
                            let secs = self.dlg_silence_secs;
                            if let Some(doc) = self.active_doc_mut() {
                                let frames = doc.buffer.secs_to_frame(secs);
                                let at = doc.cursor_frame;
                                let next = insert_silence(&doc.buffer, at, frames);
                                doc.annotations.adjust_for_edit(at, at, frames);
                                doc.commit_buffer(next, at, Some((at, at + frames)));
                            }
                            self.dialog = ActiveDialog::None;
                        }
                    });
                if !open {
                    self.dialog = ActiveDialog::None;
                }
            }

            ActiveDialog::Export => {
                let mut open = true;
                egui::Window::new("Exportar Áudio")
                    .open(&mut open)
                    .resizable(false)
                    .show(ctx, |ui| {
                        ui.label("Formato de saída:");
                        for &fmt in ExportFormat::all() {
                            let enabled = !fmt.requires_ffmpeg() || self.ffmpeg_path.is_some();
                            ui.add_enabled_ui(enabled, |ui| {
                                ui.radio_value(&mut self.dlg_export_format, fmt, fmt.label());
                            });
                        }
                        if self.ffmpeg_path.is_none() {
                            ui.label(
                                egui::RichText::new("Instale `ffmpeg` para habilitar MP3/FLAC/OGG/M4A.")
                                    .color(ForgeTheme::ACCENT_AMBER),
                            );
                        }
                        ui.separator();
                        ui.horizontal(|ui| {
                            ui.label("Taxa de Amostragem:");
                            ui.radio_value(&mut self.dlg_export_sr, None, "Original");
                            ui.radio_value(&mut self.dlg_export_sr, Some(44100), "44100 Hz");
                            ui.radio_value(&mut self.dlg_export_sr, Some(48000), "48000 Hz");
                        });
                        ui.add_space(6.0);
                        ui.horizontal(|ui| {
                            if ui.button("📤 Escolher Destino e Exportar...").clicked() {
                                self.run_export();
                            }
                            if ui.button("Cancelar").clicked() {
                                self.dialog = ActiveDialog::None;
                            }
                        });
                    });
                if !open {
                    self.dialog = ActiveDialog::None;
                }
            }

            ActiveDialog::Shortcuts => {
                let mut open = true;
                egui::Window::new("Atalhos de Teclado (Estilo Sound Forge)")
                    .open(&mut open)
                    .resizable(false)
                    .show(ctx, |ui| {
                        let rows = [
                            ("Espaço", "Tocar / Pausar"),
                            ("Enter", "Parar e voltar ao ponto inicial"),
                            ("M", "Inserir Marcador no cursor"),
                            ("R", "Criar Região a partir da seleção"),
                            ("L", "Alternar Loop na seleção"),
                            ("Z", "Alternar Snap Zero-Crossing"),
                            ("Duplo Clique", "Selecionar entre marcadores ou tudo"),
                            ("Scroll Mouse", "Zoom horizontal na posição do cursor"),
                            ("Shift + Scroll / Meio", "Mover (pan) pela onda"),
                            ("Ctrl+X / C / V", "Recortar / Copiar / Colar"),
                            ("Ctrl+T", "Aparar para seleção (Trim/Crop)"),
                            ("Ctrl+Z / Ctrl+Y", "Desfazer / Refazer"),
                            ("Ctrl+0", "Mostrar música inteira (Fit All)"),
                        ];
                        egui::Grid::new("shortcuts_grid").striped(true).show(ui, |ui| {
                            for (k, d) in rows {
                                ui.label(egui::RichText::new(k).monospace().strong());
                                ui.label(d);
                                ui.end_row();
                            }
                        });
                    });
                if !open {
                    self.dialog = ActiveDialog::None;
                }
            }

            ActiveDialog::About => {
                let mut open = true;
                egui::Window::new("Sobre — Sound Forge Linux")
                    .open(&mut open)
                    .resizable(false)
                    .show(ctx, |ui| {
                        ui.heading("Sound Forge Linux v0.1.0");
                        ui.label("Reconstrução clean-room em Rust nativo inspirada no Sound Forge Pro 10.0.");
                        ui.add_space(4.0);
                        ui.label("• Decodificação nativa (MP3, WAV, FLAC, OGG, M4A) via Symphonia");
                        ui.label("• Time-Stretch (WSOLA) & Varispeed para acelerar/ralentar músicas");
                        ui.label("• Marcadores, Regiões, Zero-Crossing, Crossfade Extend, VU Estéreo");
                        ui.label("• Áudio em tempo real via CPAL (ALSA/PulseAudio/PipeWire no Linux)");
                    });
                if !open {
                    self.dialog = ActiveDialog::None;
                }
            }
        }
    }
}
