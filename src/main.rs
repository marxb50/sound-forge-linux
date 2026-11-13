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
    apply_3band_eq, apply_delay_echo, apply_gain_db, apply_reverb, auto_trim_crop,
    change_speed_range_with_algo, concat_with_crossfade, convert_channels, delete_range,
    extend_range_with_crossfade, fade_in, fade_out, graphic_fade, insert_buffer, insert_silence,
    invert_flip_range, mix_buffer, normalize_audio, pan_expand_range, pitch_shift_range_ex,
    remove_dc_offset, resample_to_rate, reverse_range, rotate_audio_range, silence_range,
    slice_frames, smooth_enhance_range, ChannelConvertMode, FadeCurve, SpeedMode,
    TimeStretchAlgorithm,
};
use crate::playback::PlaybackEngine;
use crate::theme::{apply_forge_style, format_timecode, ForgeTheme, UiSkin};
use crate::waveform_ui::{
    draw_sound_forge_doc_window, draw_vu_meter, ActiveTool, DocWindowAction, WaveformInteraction,
};
use eframe::egui;
use std::path::PathBuf;

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Sony Sound Forge Pro 10.0 (Clean-Room Linux Edition)")
            .with_inner_size([1380.0, 850.0])
            .with_min_inner_size([960.0, 580.0])
            .with_drag_and_drop(true),
        ..Default::default()
    };

    eframe::run_native(
        "Sound Forge Linux",
        options,
        Box::new(|cc| {
            let skin = UiSkin::ClassicSoundForge10;
            apply_forge_style(&cc.egui_ctx, skin);
            Ok(Box::new(ForgeApp::new(skin)))
        }),
    )
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MdiLayoutMode {
    /// Stacked horizontally (one song above another — default Sound Forge multi-song view in Screenshot 1!)
    TileHorizontally,
    /// Side-by-side columns
    TileVertically,
    /// Single active maximized window with tabs
    MaximizedTabs,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TimeStretchInputFormat {
    TimeHms,
    Percent,
    TempoBpm,
}

impl TimeStretchInputFormat {
    fn label(&self) -> &'static str {
        match self {
            TimeStretchInputFormat::TimeHms => "Time (hr:mn:sc.xxx)",
            TimeStretchInputFormat::Percent => "Percent (% of original)",
            TimeStretchInputFormat::TempoBpm => "Tempo (BPM)",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ActiveDialog {
    None,
    Volume,
    TimeStretch,
    GraphicFade,
    Normalize,
    AutoTrimCrop,
    ChannelConverter,
    PanExpand,
    SmoothEnhance,
    Eq3Band,
    PitchShift,
    DelayEcho,
    Reverb,
    ExtendSelection,
    InsertSilence,
    SendToOtherDoc,
    JoinOpenSongs,
    Export,
    Shortcuts,
    About,
}

struct ForgeApp {
    skin: UiSkin,
    mdi_layout: MdiLayoutMode,
    active_tool: ActiveTool,

    docs: Vec<AudioDocument>,
    active_tab: usize,
    playing_doc_idx: Option<usize>,
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
    dlg_bypass: bool,
    dlg_realtime: bool,

    // Volume dialog (Screenshot 3)
    dlg_vol_preset: String,
    dlg_gain_db: f32,

    // Sony Time Stretch dialog (Screenshot 4)
    dlg_ts_preset: String,
    dlg_ts_algo: TimeStretchAlgorithm,
    dlg_ts_format: TimeStretchInputFormat,
    dlg_ts_percent_of_orig: f64, // 100% = same, 89.85% = shorter/faster, 115% = longer/slower
    dlg_ts_final_time_str: String,
    dlg_ts_bpm_from: f64,
    dlg_ts_bpm_to: f64,

    // Graphic Fade dialog
    dlg_fade_start_pct: f32,
    dlg_fade_end_pct: f32,
    dlg_fade_curve: FadeCurve,

    // Normalize dialog
    dlg_norm_db: f32,
    dlg_norm_rms: bool,

    // Auto Trim/Crop dialog
    dlg_trim_thresh_db: f32,

    // Channel Converter dialog
    dlg_chan_mode: ChannelConvertMode,

    // Pan/Expand dialog
    dlg_pan: f32,
    dlg_stereo_width: f32,

    // Smooth/Enhance dialog
    dlg_smooth_amount: i32,

    // 3-Band EQ dialog
    dlg_eq_low_db: f32,
    dlg_eq_mid_db: f32,
    dlg_eq_high_db: f32,

    // Pitch Shift dialog
    dlg_pitch_semitones: f64,
    dlg_pitch_cents: f64,
    dlg_pitch_preserve_dur: bool,

    // Delay / Reverb dialogs
    dlg_delay_ms: f32,
    dlg_delay_fb: f32,
    dlg_delay_wet: f32,
    dlg_rev_room: f32,
    dlg_rev_damp: f32,
    dlg_rev_wet: f32,

    // Extend / Silence
    dlg_extend_secs: f64,
    dlg_extend_xfade_ms: f64,
    dlg_silence_secs: f64,

    // Multi-song transfer & join
    dlg_target_doc_idx: usize,
    dlg_transfer_mode: usize, // 0 = Insert at Target Cursor, 1 = Append at End with Crossfade, 2 = Mix at Target Cursor
    dlg_join_xfade_secs: f64,

    // Export
    dlg_export_format: ExportFormat,
    dlg_export_sr: Option<u32>,

    // Panels & status
    show_markers_panel: bool,
    show_meter_panel: bool,
    ffmpeg_path: Option<PathBuf>,
    status_msg: String,
}

impl ForgeApp {
    fn new(skin: UiSkin) -> Self {
        let ffmpeg_path = find_ffmpeg();
        let status_msg = if ffmpeg_path.is_some() {
            "Pronto — Layout Sound Forge Pro 10.0 ativo (MDI Multi-Músicas + FFmpeg detectado)."
                .to_string()
        } else {
            "Pronto — Layout Sound Forge Pro 10.0 ativo (WAV nativo; instale `ffmpeg` p/ MP3/FLAC)."
                .to_string()
        };

        let mut app = Self {
            skin,
            mdi_layout: MdiLayoutMode::TileHorizontally,
            active_tool: ActiveTool::EditSelect,
            docs: Vec::new(),
            active_tab: 0,
            playing_doc_idx: None,
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
            dlg_bypass: false,
            dlg_realtime: true,
            dlg_vol_preset: "(Untitled)".to_string(),
            dlg_gain_db: 0.0,
            dlg_ts_preset: "(Untitled)".to_string(),
            dlg_ts_algo: TimeStretchAlgorithm::Music1MinFlange,
            dlg_ts_format: TimeStretchInputFormat::TimeHms,
            dlg_ts_percent_of_orig: 100.0,
            dlg_ts_final_time_str: String::new(),
            dlg_ts_bpm_from: 120.0,
            dlg_ts_bpm_to: 120.0,
            dlg_fade_start_pct: 100.0,
            dlg_fade_end_pct: 0.0,
            dlg_fade_curve: FadeCurve::SmoothSCurve,
            dlg_norm_db: -0.5,
            dlg_norm_rms: false,
            dlg_trim_thresh_db: -42.0,
            dlg_chan_mode: ChannelConvertMode::StereoToMonoMix,
            dlg_pan: 0.0,
            dlg_stereo_width: 1.0,
            dlg_smooth_amount: 0,
            dlg_eq_low_db: 0.0,
            dlg_eq_mid_db: 0.0,
            dlg_eq_high_db: 0.0,
            dlg_pitch_semitones: 0.0,
            dlg_pitch_cents: 0.0,
            dlg_pitch_preserve_dur: true,
            dlg_delay_ms: 280.0,
            dlg_delay_fb: 0.35,
            dlg_delay_wet: 0.30,
            dlg_rev_room: 0.65,
            dlg_rev_damp: 0.40,
            dlg_rev_wet: 0.28,
            dlg_extend_secs: 2.5,
            dlg_extend_xfade_ms: 25.0,
            dlg_silence_secs: 1.0,
            dlg_target_doc_idx: 0,
            dlg_transfer_mode: 0,
            dlg_join_xfade_secs: 1.5,
            dlg_export_format: ExportFormat::Wav24,
            dlg_export_sr: None,
            show_markers_panel: false,
            show_meter_panel: true,
            ffmpeg_path,
            status_msg,
        };

        for arg_path in std::env::args().skip(1) {
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
                "Áudio (MP3, WAV, FLAC, OGG, M4A)",
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
                    .unwrap_or("Audio.mp3")
                    .to_string();
                let id = self.next_doc_id;
                self.next_doc_id += 1;
                let doc = AudioDocument::from_buffer(id, title.clone(), Some(path), buf);
                self.playback.set_buffer(doc.buffer.clone());
                self.docs.push(doc);
                self.active_tab = self.docs.len() - 1;
                self.status_msg = format!(
                    "Aberta: {title} ({} música(s) na bancada MDI)",
                    self.docs.len()
                );
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
        let doc = AudioDocument::from_buffer(id, format!("Sound {id}.wav"), None, buf);
        self.docs.push(doc);
        self.active_tab = self.docs.len() - 1;
        self.status_msg = "Nova janela de áudio estéreo 44.1 kHz (5s) criada.".to_string();
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

    fn open_time_stretch_dialog(&mut self) {
        let init_secs = self.active_target_duration_secs();
        self.dlg_ts_percent_of_orig = (100.0 / self.playback_speed.max(0.25)).clamp(25.0, 400.0);
        let final_secs = init_secs * (self.dlg_ts_percent_of_orig / 100.0);
        self.dlg_ts_final_time_str = format_timecode(final_secs);
        self.dialog = ActiveDialog::TimeStretch;
    }

    fn active_target_duration_secs(&self) -> f64 {
        if let Some(doc) = self.active_doc() {
            let (s, e) = doc.target_range();
            doc.buffer.frame_to_secs(e.saturating_sub(s)).max(0.01)
        } else {
            1.0
        }
    }

    fn toggle_play(&mut self) {
        if self.playback.is_playing() {
            let cur = self.playback.current_source_frame();
            let p_idx = self.playing_doc_idx.unwrap_or(self.active_tab);
            if let Some(doc) = self.docs.get_mut(p_idx) {
                doc.cursor_frame = cur;
            }
            self.playback.pause();
            self.playing_doc_idx = None;
            return;
        }
        self.play_doc(self.active_tab, false);
    }

    fn play_doc(&mut self, doc_idx: usize, force_from_cursor: bool) {
        let Some(doc) = self.docs.get(doc_idx) else {
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

        // Combine global playback_speed with per-window `Rate` slider if non-zero!
        let rate_factor = doc.rate_to_speed_factor();
        let effective_speed = (self.playback_speed * rate_factor).clamp(0.25, 4.0);
        let effective_mode = if doc.rate_slider.abs() > 0.01 {
            SpeedMode::Varispeed
        } else {
            self.speed_mode
        };

        self.playing_doc_idx = Some(doc_idx);
        self.playback.play_range(
            doc.buffer.clone(),
            start,
            end,
            self.loop_enabled,
            effective_speed,
            effective_mode,
        );
    }

    /// Plays a temporary preview buffer (for Dialog `[ Preview ]` buttons like Volume & Time Stretch!).
    fn preview_processed_buffer(&mut self, preview_buf: AudioBuffer) {
        let frames = preview_buf.num_frames();
        if frames == 0 {
            return;
        }
        // Respect 10-second preview limit if not looped, or play full selection
        let max_preview_frames = (preview_buf.sample_rate as usize * 15).min(frames);
        self.playing_doc_idx = None;
        self.playback.play_range(
            std::sync::Arc::new(preview_buf),
            0,
            max_preview_frames,
            false,
            1.0,
            SpeedMode::PreservePitch,
        );
    }

    fn stop_playback(&mut self) {
        self.playback.stop();
        self.playing_doc_idx = None;
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
        let title = doc.title.clone();
        self.clipboard = Some(copied);
        self.status_msg = format!(
            "Copiado de '{title}': {} (você pode clicar em outra música e teclar Ctrl+V!).",
            format_timecode(secs)
        );
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
        self.status_msg = "Trecho colado na música ativa.".to_string();
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
        doc.annotations
            .markers
            .retain(|m| m.frame >= s && m.frame <= e);
        for m in &mut doc.annotations.markers {
            m.frame -= s;
        }
        doc.annotations
            .regions
            .retain(|r| r.end_frame > s && r.start_frame < e);
        for r in &mut doc.annotations.regions {
            r.start_frame = r.start_frame.saturating_sub(s);
            r.end_frame = r.end_frame.saturating_sub(s).min(new_len);
        }
        doc.commit_buffer(next_buf, 0, None);
        doc.zoom_fit();
        self.status_msg = "Arquivo aparado para a seleção (Trim/Crop).".to_string();
    }

    fn convert_markers_to_regions(&mut self) {
        let Some(doc) = self.active_doc_mut() else {
            return;
        };
        if doc.annotations.markers.len() < 2 {
            self.status_msg = "Adicione pelo menos 2 marcadores (M) para converter em regiões."
                .to_string();
            return;
        }
        let frames: Vec<(usize, String)> = doc
            .annotations
            .markers
            .iter()
            .map(|m| (m.frame, m.name.clone()))
            .collect();
        for w in frames.windows(2) {
            let (f0, ref n0) = w[0];
            let (f1, _) = w[1];
            if f1 > f0 {
                doc.annotations
                    .add_region(f0, f1, Some(format!("Trecho {n0}")));
            }
        }
        self.status_msg = "Marcadores convertidos em Regiões.".to_string();
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
        if ctx.input(|i| i.key_pressed(egui::Key::M)) && !ctrl {
            if let Some(doc) = self.active_doc_mut() {
                doc.annotations.add_marker(doc.cursor_frame, None);
                self.status_msg = "Marcador inserido no cursor.".to_string();
            }
        }
        if ctx.input(|i| i.key_pressed(egui::Key::R)) && !ctrl {
            if let Some(doc) = self.active_doc_mut() {
                if let Some((s, e)) = doc.selection {
                    doc.annotations.add_region(s, e, None);
                    self.status_msg = "Região criada a partir da seleção.".to_string();
                }
            }
        }
        if ctx.input(|i| i.key_pressed(egui::Key::L)) && !ctrl {
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
            if ctx.input(|i| i.key_pressed(egui::Key::N)) {
                self.new_empty_document();
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
            if ctx.input(|i| i.key_pressed(egui::Key::M)) {
                self.action_paste_mix();
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
        } else {
            self.playing_doc_idx = None;
        }

        // 1. Authentic Sound Forge Pro 10.0 Menu Bar
        egui::TopBottomPanel::top("sf10_menu_bar").show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                ui.menu_button("File", |ui| {
                    if ui.button("New (Ctrl+N)").clicked() {
                        self.new_empty_document();
                        ui.close_menu();
                    }
                    if ui.button("Open... (Ctrl+O)").clicked() {
                        self.open_file_dialog();
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui.button("Save WAV (Ctrl+S)").clicked() {
                        self.save_active_wav();
                        ui.close_menu();
                    }
                    if ui.button("Save As / Render As (MP3, FLAC, WAV)...").clicked() {
                        self.dialog = ActiveDialog::Export;
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui.button("Close Active Song").clicked() {
                        if !self.docs.is_empty() {
                            self.stop_playback();
                            self.docs.remove(self.active_tab);
                            self.active_tab = self.active_tab.min(self.docs.len().saturating_sub(1));
                        }
                        ui.close_menu();
                    }
                    if ui.button("Close All").clicked() {
                        self.stop_playback();
                        self.docs.clear();
                        self.active_tab = 0;
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui.button("Exit").clicked() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                });

                ui.menu_button("Edit", |ui| {
                    if ui.button("Undo (Ctrl+Z)").clicked() {
                        if let Some(doc) = self.active_doc_mut() {
                            doc.undo();
                        }
                        ui.close_menu();
                    }
                    if ui.button("Redo (Ctrl+Y)").clicked() {
                        if let Some(doc) = self.active_doc_mut() {
                            doc.redo();
                        }
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui.button("Cut (Ctrl+X)").clicked() {
                        self.action_cut();
                        ui.close_menu();
                    }
                    if ui.button("Copy (Ctrl+C)").clicked() {
                        self.action_copy();
                        ui.close_menu();
                    }
                    if ui.button("Paste (Ctrl+V)").clicked() {
                        self.action_paste();
                        ui.close_menu();
                    }
                    if ui.button("Paste Special -> Mix (Ctrl+M)").clicked() {
                        self.action_paste_mix();
                        ui.close_menu();
                    }
                    if ui.button("Send Selection to Another Open Song...").clicked() {
                        self.dialog = ActiveDialog::SendToOtherDoc;
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui.button("Delete (Clear) (Del)").clicked() {
                        self.action_delete();
                        ui.close_menu();
                    }
                    if ui.button("Trim/Crop (Ctrl+T)").clicked() {
                        self.action_trim_crop();
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui.button("Select All (Ctrl+A)").clicked() {
                        if let Some(doc) = self.active_doc_mut() {
                            let n = doc.buffer.num_frames();
                            if n > 0 {
                                doc.selection = Some((0, n));
                            }
                        }
                        ui.close_menu();
                    }
                    ui.menu_button("Tool", |ui| {
                        ui.radio_value(
                            &mut self.active_tool,
                            ActiveTool::EditSelect,
                            "Edit Tool (I-Beam Selection)",
                        );
                        ui.radio_value(
                            &mut self.active_tool,
                            ActiveTool::MagnifyZoom,
                            "Magnify Tool (Zoom Box)",
                        );
                        ui.radio_value(
                            &mut self.active_tool,
                            ActiveTool::PencilDraw,
                            "Pencil Tool (Draw Samples)",
                        );
                    });
                });

                ui.menu_button("View", |ui| {
                    if ui.button("Maximize Width (Fit All) (Ctrl+0)").clicked() {
                        if let Some(doc) = self.active_doc_mut() {
                            doc.zoom_fit();
                        }
                        ui.close_menu();
                    }
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
                    if ui.button("Zoom to Selection").clicked() {
                        if let Some(doc) = self.active_doc_mut() {
                            doc.zoom_selection();
                        }
                        ui.close_menu();
                    }
                    ui.separator();
                    ui.checkbox(&mut self.show_markers_panel, "Regions / Markers List");
                    ui.checkbox(&mut self.show_meter_panel, "Play Meters (VU)");
                });

                ui.menu_button("Insert", |ui| {
                    if ui.button("Marker (M)").clicked() {
                        if let Some(doc) = self.active_doc_mut() {
                            doc.annotations.add_marker(doc.cursor_frame, None);
                        }
                        ui.close_menu();
                    }
                    if ui.button("Region (R)").clicked() {
                        if let Some(doc) = self.active_doc_mut() {
                            if let Some((s, e)) = doc.selection {
                                doc.annotations.add_region(s, e, None);
                            }
                        }
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui.button("Silence...").clicked() {
                        self.dialog = ActiveDialog::InsertSilence;
                        ui.close_menu();
                    }
                });

                // Exact Process Menu from Screenshot 2!
                ui.menu_button("Process", |ui| {
                    if ui.button("Auto Trim/Crop...").clicked() {
                        self.dialog = ActiveDialog::AutoTrimCrop;
                        ui.close_menu();
                    }
                    ui.menu_button("Bit Depth", |ui| {
                        for label in ["16-bit PCM", "24-bit PCM", "32-bit IEEE Float"] {
                            if ui.button(label).clicked() {
                                self.status_msg = format!("Profundidade configurada: {label}.");
                                ui.close_menu();
                            }
                        }
                    });
                    if ui.button("Channel Converter...").clicked() {
                        self.dialog = ActiveDialog::ChannelConverter;
                        ui.close_menu();
                    }
                    if ui.button("DC Offset...").clicked() {
                        self.stop_playback();
                        if let Some(doc) = self.active_doc_mut() {
                            let (s, e) = doc.target_range();
                            let (next, dc) = remove_dc_offset(&doc.buffer, s, e);
                            doc.commit_buffer(next, doc.cursor_frame, doc.selection);
                            self.status_msg =
                                format!("DC Offset removido ({:+.4}% corrigido).", dc * 100.0);
                        }
                        ui.close_menu();
                    }
                    ui.menu_button("EQ", |ui| {
                        if ui.button("Graphic / 3-Band EQ...").clicked() {
                            self.dialog = ActiveDialog::Eq3Band;
                            ui.close_menu();
                        }
                        if ui.button("Paragraphic...").clicked() {
                            self.dialog = ActiveDialog::Eq3Band;
                            ui.close_menu();
                        }
                        if ui.button("Parametric...").clicked() {
                            self.dialog = ActiveDialog::Eq3Band;
                            ui.close_menu();
                        }
                    });
                    ui.menu_button("Fade", |ui| {
                        if ui.button("Graphic...").clicked() {
                            self.dialog = ActiveDialog::GraphicFade;
                            ui.close_menu();
                        }
                        if ui.button("In").clicked() {
                            self.stop_playback();
                            if let Some(doc) = self.active_doc_mut() {
                                let (s, e) = doc.target_range();
                                let next = fade_in(&doc.buffer, s, e, FadeCurve::Linear);
                                doc.commit_buffer(next, doc.cursor_frame, doc.selection);
                            }
                            self.status_msg = "Fade In aplicado.".to_string();
                            ui.close_menu();
                        }
                        if ui.button("Out").clicked() {
                            self.stop_playback();
                            if let Some(doc) = self.active_doc_mut() {
                                let (s, e) = doc.target_range();
                                let next = fade_out(&doc.buffer, s, e, FadeCurve::Linear);
                                doc.commit_buffer(next, doc.cursor_frame, doc.selection);
                            }
                            self.status_msg = "Fade Out aplicado.".to_string();
                            ui.close_menu();
                        }
                    });
                    if ui.button("Invert/Flip").clicked() {
                        self.stop_playback();
                        if let Some(doc) = self.active_doc_mut() {
                            let (s, e) = doc.target_range();
                            let next = invert_flip_range(&doc.buffer, s, e);
                            doc.commit_buffer(next, doc.cursor_frame, doc.selection);
                        }
                        self.status_msg = "Fase invertida (Invert/Flip).".to_string();
                        ui.close_menu();
                    }
                    if ui.button("Mute").clicked() {
                        self.stop_playback();
                        if let Some(doc) = self.active_doc_mut() {
                            let (s, e) = doc.target_range();
                            let next = silence_range(&doc.buffer, s, e);
                            doc.commit_buffer(next, doc.cursor_frame, doc.selection);
                        }
                        self.status_msg = "Trecho silenciado (Mute).".to_string();
                        ui.close_menu();
                    }
                    if ui.button("Normalize...").clicked() {
                        self.dialog = ActiveDialog::Normalize;
                        ui.close_menu();
                    }
                    if ui.button("Pan/Expand...").clicked() {
                        self.dialog = ActiveDialog::PanExpand;
                        ui.close_menu();
                    }
                    ui.menu_button("Resample", |ui| {
                        for &sr in &[22050_u32, 44100, 48000, 96000] {
                            if ui.button(format!("{sr} Hz")).clicked() {
                                self.stop_playback();
                                if let Some(doc) = self.active_doc_mut() {
                                    let next = resample_to_rate(&doc.buffer, sr);
                                    doc.commit_buffer(next, 0, None);
                                    doc.zoom_fit();
                                }
                                self.status_msg = format!("Resample concluído para {sr} Hz.");
                                ui.close_menu();
                            }
                        }
                    });
                    if ui.button("Reverse").clicked() {
                        self.stop_playback();
                        if let Some(doc) = self.active_doc_mut() {
                            let (s, e) = doc.target_range();
                            let next = reverse_range(&doc.buffer, s, e);
                            doc.commit_buffer(next, doc.cursor_frame, doc.selection);
                        }
                        self.status_msg = "Áudio invertido (Reverse).".to_string();
                        ui.close_menu();
                    }
                    if ui.button("Rotate Audio").clicked() {
                        self.stop_playback();
                        if let Some(doc) = self.active_doc_mut() {
                            let (s, e) = doc.target_range();
                            let quarter = (e.saturating_sub(s)) / 4;
                            let next = rotate_audio_range(&doc.buffer, s, e, quarter);
                            doc.commit_buffer(next, doc.cursor_frame, doc.selection);
                        }
                        self.status_msg = "Áudio rotacionado em 25% do trecho.".to_string();
                        ui.close_menu();
                    }
                    if ui.button("Smooth/Enhance...").clicked() {
                        self.dialog = ActiveDialog::SmoothEnhance;
                        ui.close_menu();
                    }
                    ui.menu_button("Time", |ui| {
                        if ui.button("Time Stretch...").clicked() {
                            self.open_time_stretch_dialog();
                            ui.close_menu();
                        }
                        if ui.button("Extend Musical Phrase (Crossfade)...").clicked() {
                            self.dialog = ActiveDialog::ExtendSelection;
                            ui.close_menu();
                        }
                    });
                    if ui.button("Volume...").clicked() {
                        self.dialog = ActiveDialog::Volume;
                        ui.close_menu();
                    }
                });

                ui.menu_button("Effects", |ui| {
                    ui.menu_button("Pitch", |ui| {
                        if ui.button("Shift...").clicked() {
                            self.dialog = ActiveDialog::PitchShift;
                            ui.close_menu();
                        }
                    });
                    if ui.button("Delay/Echo...").clicked() {
                        self.dialog = ActiveDialog::DelayEcho;
                        ui.close_menu();
                    }
                    if ui.button("Reverb...").clicked() {
                        self.dialog = ActiveDialog::Reverb;
                        ui.close_menu();
                    }
                });

                ui.menu_button("Tools", |ui| {
                    if ui.button("Join / Merge Open Songs...").clicked() {
                        self.dialog = ActiveDialog::JoinOpenSongs;
                        ui.close_menu();
                    }
                    if ui.button("Send Selection to Another Song...").clicked() {
                        self.dialog = ActiveDialog::SendToOtherDoc;
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui.button("Convert Markers to Regions").clicked() {
                        self.convert_markers_to_regions();
                        ui.close_menu();
                    }
                });

                ui.menu_button("FX Favorites", |ui| {
                    if ui.button("Sony Time Stretch...").clicked() {
                        self.open_time_stretch_dialog();
                        ui.close_menu();
                    }
                    if ui.button("Volume (-Inf to +20 dB)...").clicked() {
                        self.dialog = ActiveDialog::Volume;
                        ui.close_menu();
                    }
                    if ui.button("Graphic Fade...").clicked() {
                        self.dialog = ActiveDialog::GraphicFade;
                        ui.close_menu();
                    }
                    if ui.button("Normalize Peak (-0.5 dB)...").clicked() {
                        self.dialog = ActiveDialog::Normalize;
                        ui.close_menu();
                    }
                });

                ui.menu_button("Options", |ui| {
                    ui.checkbox(
                        &mut self.snap_zero_crossing,
                        "Auto Snap to Zero-Crossing (Z)",
                    );
                    ui.separator();
                    ui.label("Visual Skin:");
                    if ui
                        .radio_value(
                            &mut self.skin,
                            UiSkin::ClassicSoundForge10,
                            "Classic Sound Forge Pro 10.0 (Silver / Navy)",
                        )
                        .changed()
                    {
                        apply_forge_style(ctx, self.skin);
                    }
                    if ui
                        .radio_value(
                            &mut self.skin,
                            UiSkin::DarkStudio,
                            "Dark Studio (Emerald / Charcoal)",
                        )
                        .changed()
                    {
                        apply_forge_style(ctx, self.skin);
                    }
                });

                ui.menu_button("Window", |ui| {
                    if ui
                        .radio_value(
                            &mut self.mdi_layout,
                            MdiLayoutMode::TileHorizontally,
                            "Tile Horizontally (Empilhar Músicas — Padrão SF10)",
                        )
                        .clicked()
                    {
                        for d in &mut self.docs {
                            d.minimized = false;
                        }
                        ui.close_menu();
                    }
                    if ui
                        .radio_value(
                            &mut self.mdi_layout,
                            MdiLayoutMode::TileVertically,
                            "Tile Vertically (Músicas Lado a Lado)",
                        )
                        .clicked()
                    {
                        for d in &mut self.docs {
                            d.minimized = false;
                        }
                        ui.close_menu();
                    }
                    if ui
                        .radio_value(
                            &mut self.mdi_layout,
                            MdiLayoutMode::MaximizedTabs,
                            "Maximize Active Window (Abas)",
                        )
                        .clicked()
                    {
                        ui.close_menu();
                    }
                    if !self.docs.is_empty() {
                        ui.separator();
                        for (i, d) in self.docs.iter().enumerate() {
                            if ui
                                .selectable_label(i == self.active_tab, format!("{}. {}", i + 1, d.title))
                                .clicked()
                            {
                                self.active_tab = i;
                                ui.close_menu();
                            }
                        }
                    }
                });

                ui.menu_button("Help", |ui| {
                    if ui.button("Keyboard Shortcuts").clicked() {
                        self.dialog = ActiveDialog::Shortcuts;
                        ui.close_menu();
                    }
                    if ui.button("About Sound Forge Linux").clicked() {
                        self.dialog = ActiveDialog::About;
                        ui.close_menu();
                    }
                });
            });
        });

        // 2. Row 1: Standard Action & Edit Tool Toolbar (matching Screenshot 1 & 2!)
        egui::TopBottomPanel::top("sf10_standard_toolbar").show(ctx, |ui| {
            ui.horizontal_wrapped(|ui| {
                if ui.button("📄").on_hover_text("New (Ctrl+N)").clicked() {
                    self.new_empty_document();
                }
                if ui
                    .button("📂 Abrir")
                    .on_hover_text("Abrir uma ou várias músicas (Ctrl+O)")
                    .clicked()
                {
                    self.open_file_dialog();
                }
                if ui.button("💾").on_hover_text("Save WAV (Ctrl+S)").clicked() {
                    self.save_active_wav();
                }
                if ui
                    .button("📤 Render/Export")
                    .on_hover_text("Exportar MP3 / FLAC / WAV")
                    .clicked()
                {
                    self.dialog = ActiveDialog::Export;
                }
                ui.separator();
                if ui.button("✂").on_hover_text("Cut (Ctrl+X)").clicked() {
                    self.action_cut();
                }
                if ui.button("📋").on_hover_text("Copy (Ctrl+C)").clicked() {
                    self.action_copy();
                }
                if ui.button("📌").on_hover_text("Paste (Ctrl+V)").clicked() {
                    self.action_paste();
                }
                if ui
                    .button("🔀 Mix")
                    .on_hover_text("Paste Mix (Ctrl+M)")
                    .clicked()
                {
                    self.action_paste_mix();
                }
                if ui
                    .button("🔪 Trim")
                    .on_hover_text("Trim/Crop to Selection (Ctrl+T)")
                    .clicked()
                {
                    self.action_trim_crop();
                }
                ui.separator();
                if ui.button("↩").on_hover_text("Undo (Ctrl+Z)").clicked() {
                    if let Some(doc) = self.active_doc_mut() {
                        doc.undo();
                    }
                }
                if ui.button("↪").on_hover_text("Redo (Ctrl+Y)").clicked() {
                    if let Some(doc) = self.active_doc_mut() {
                        doc.redo();
                    }
                }
                ui.separator();
                // Sound Forge Tool Icons (Edit I-Beam, Magnify, Pencil)
                ui.selectable_value(&mut self.active_tool, ActiveTool::EditSelect, "┼ Edit")
                    .on_hover_text("Ferramenta de Edição / Seleção (padrão)");
                ui.selectable_value(&mut self.active_tool, ActiveTool::MagnifyZoom, "🔍 Zoom")
                    .on_hover_text("Ferramenta Lupa (arraste para dar zoom no trecho)");
                ui.selectable_value(&mut self.active_tool, ActiveTool::PencilDraw, "✏ Lápis")
                    .on_hover_text("Ferramenta Lápis (desenhar amostras quando aproximado)");
                ui.separator();
                if ui.button("🔊 Volume").clicked() {
                    self.dialog = ActiveDialog::Volume;
                }
                if ui.button("⏱ Time Stretch").clicked() {
                    self.open_time_stretch_dialog();
                }
                if ui.button("📉 Fade").clicked() {
                    self.dialog = ActiveDialog::GraphicFade;
                }
                if ui.button("🔁 +Estender").clicked() {
                    self.dialog = ActiveDialog::ExtendSelection;
                }
                if self.docs.len() >= 2 {
                    if ui
                        .button("🔗 Juntar Músicas")
                        .on_hover_text("Concatenar ou mesclar as músicas abertas com crossfade")
                        .clicked()
                    {
                        self.dialog = ActiveDialog::JoinOpenSongs;
                    }
                }
                ui.separator();
                // Multi-window layout switcher
                ui.selectable_value(
                    &mut self.mdi_layout,
                    MdiLayoutMode::TileHorizontally,
                    "▤ Empilhar",
                )
                .on_hover_text("Mostrar várias músicas empilhadas (como no Sound Forge 10)");
                ui.selectable_value(
                    &mut self.mdi_layout,
                    MdiLayoutMode::TileVertically,
                    "▥ Lado a Lado",
                );
                ui.selectable_value(
                    &mut self.mdi_layout,
                    MdiLayoutMode::MaximizedTabs,
                    "🗖 Abas",
                );
            });
        });

        // 3. Row 2: Global Transport & Speed Toolbar (Screenshot 1 & 2!)
        egui::TopBottomPanel::top("sf10_transport_toolbar").show(ctx, |ui| {
            ui.horizontal_wrapped(|ui| {
                if ui
                    .add(egui::Button::new(
                        egui::RichText::new("⏺").color(ForgeTheme::ACCENT_RED),
                    ))
                    .on_hover_text("Criar Nova Janela de Gravação / Áudio")
                    .clicked()
                {
                    self.new_empty_document();
                }
                if ui
                    .toggle_value(&mut self.loop_enabled, "🔄 Loop")
                    .clicked()
                {
                    self.playback.set_loop(self.loop_enabled);
                }
                let playing = self.playback.is_playing();
                if ui
                    .button("▶| Play All")
                    .on_hover_text("Tocar música inteira desde o início")
                    .clicked()
                {
                    if let Some(doc) = self.active_doc_mut() {
                        doc.cursor_frame = 0;
                        doc.selection = None;
                    }
                    self.play_doc(self.active_tab, true);
                }
                if ui
                    .button(if playing { "⏸ Pause" } else { "▶ Play" })
                    .clicked()
                {
                    self.toggle_play();
                }
                if ui.button("■ Stop").clicked() {
                    self.stop_playback();
                }
                if ui.button("|◀").on_hover_text("Início (Home)").clicked() {
                    if let Some(doc) = self.active_doc_mut() {
                        doc.cursor_frame = 0;
                    }
                }
                if ui.button("▶|").on_hover_text("Fim (End)").clicked() {
                    if let Some(doc) = self.active_doc_mut() {
                        doc.cursor_frame = doc.buffer.num_frames();
                    }
                }

                ui.separator();
                ui.label("Velocidade:");
                let speed_changed = ui
                    .add(
                        egui::Slider::new(&mut self.playback_speed, 0.50..=2.00)
                            .fixed_decimals(2)
                            .suffix("x"),
                    )
                    .changed();

                for &(preset, lbl) in &[
                    (0.80, "80%"),
                    (0.90, "90%"),
                    (1.00, "100%"),
                    (1.10, "110%"),
                    (1.20, "120%"),
                ] {
                    if ui
                        .selectable_label((self.playback_speed - preset).abs() < 0.005, lbl)
                        .clicked()
                    {
                        self.playback_speed = preset;
                        if playing {
                            self.play_doc(self.active_tab, false);
                        }
                    }
                }

                let mut preserve_pitch = self.speed_mode == SpeedMode::PreservePitch;
                if ui.checkbox(&mut preserve_pitch, "Preservar Tom").changed() {
                    self.speed_mode = if preserve_pitch {
                        SpeedMode::PreservePitch
                    } else {
                        SpeedMode::Varispeed
                    };
                    if playing {
                        self.play_doc(self.active_tab, false);
                    }
                }
                if speed_changed && playing && self.speed_mode == SpeedMode::Varispeed {
                    self.play_doc(self.active_tab, false);
                }

                if (self.playback_speed - 1.0).abs() > 0.005 {
                    if ui.button("✔ Gravar Velocidade").clicked() {
                        let spd = self.playback_speed;
                        let algo = if self.speed_mode == SpeedMode::PreservePitch {
                            TimeStretchAlgorithm::Music2Balanced
                        } else {
                            TimeStretchAlgorithm::VarispeedTape
                        };
                        self.apply_time_stretch_to_doc(spd, algo);
                        self.playback_speed = 1.0;
                    }
                }
            });
        });

        // Bottom Status Bar
        egui::TopBottomPanel::bottom("sf10_status_bar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                if let Some(doc) = self.active_doc() {
                    let sr = doc.buffer.sample_rate;
                    let ch_str = if doc.buffer.channels == 1 {
                        "Mono"
                    } else {
                        "Stereo"
                    };
                    ui.label(
                        egui::RichText::new(format!(
                            "[{}] {} Hz • 32-bit (Float) • {}",
                            doc.title, sr, ch_str
                        ))
                        .monospace()
                        .size(11.0),
                    );
                    ui.separator();
                }
                ui.label(egui::RichText::new(&self.status_msg).size(11.5));
            });
        });

        // Optional Left Dock: Regions / Markers List
        if self.show_markers_panel && !self.docs.is_empty() {
            egui::SidePanel::left("markers_panel")
                .default_width(225.0)
                .show(ctx, |ui| {
                    ui.heading(egui::RichText::new("Marcadores & Regiões").size(13.5));
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
                        egui::ScrollArea::vertical().show(ui, |ui| {
                            let mut remove_m = None;
                            let mut jump_to = None;
                            let sr = doc.buffer.sample_rate;
                            for (i, m) in doc.annotations.markers.iter_mut().enumerate() {
                                ui.horizontal(|ui| {
                                    let t = m.frame as f64 / sr as f64;
                                    if ui
                                        .button(format!("#{} {}", i + 1, format_timecode(t)))
                                        .clicked()
                                    {
                                        jump_to = Some(m.frame);
                                    }
                                    ui.add(
                                        egui::TextEdit::singleline(&mut m.name)
                                            .desired_width(78.0),
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
                        });
                    }
                });
        }

        // Right Dock: Stereo VU Meter
        if self.show_meter_panel {
            egui::SidePanel::right("meter_panel")
                .exact_width(72.0)
                .show(ctx, |ui| {
                    let meter = self.playback.meter_reading();
                    draw_vu_meter(ui, &meter, self.skin);
                });
        }

        // 4. Central MDI Workspace (Supports Multiple Open Songs Stacked, Side-by-Side, or Maximized!)
        let mdi_bg = self.skin.palette().mdi_bg;
        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(mdi_bg).inner_margin(6.0))
            .show(ctx, |ui| {
                if self.docs.is_empty() {
                    ui.centered_and_justified(|ui| {
                        ui.vertical_centered(|ui| {
                            ui.add_space(90.0);
                            ui.heading("Sony Sound Forge Pro 10.0 — Clean-Room Linux Edition");
                            ui.add_space(8.0);
                            ui.label("Abra várias músicas ao mesmo tempo (Ctrl+O ou arraste para cá) para editar empilhadas como no Sound Forge 10.");
                            ui.add_space(16.0);
                            ui.horizontal(|ui| {
                                if ui.button("📂 Abrir Músicas (MP3, WAV, FLAC, OGG)...").clicked() {
                                    self.open_file_dialog();
                                }
                                if ui.button("📄 Criar Novo Áudio").clicked() {
                                    self.new_empty_document();
                                }
                            });
                        });
                    });
                    return;
                }

                let num_docs = self.docs.len();
                let has_multi = num_docs > 1;
                let avail_h = ui.available_height().max(220.0);
                let is_playing = self.playback.is_playing();
                let cur_play_frame = if is_playing {
                    Some(self.playback.current_source_frame())
                } else {
                    None
                };

                let mut pending_action: Option<(usize, DocWindowAction)> = None;

                match self.mdi_layout {
                    MdiLayoutMode::TileHorizontally => {
                        let non_min = self.docs.iter().filter(|d| !d.minimized).count().max(1);
                        let min_count = self.docs.iter().filter(|d| d.minimized).count();
                        let usable_h = (avail_h - (min_count as f32 * 34.0) - (num_docs as f32 * 6.0)).max(150.0);
                        let each_h = (usable_h / non_min as f32).max(165.0);

                        egui::ScrollArea::vertical().show(ui, |ui| {
                            for idx in 0..num_docs {
                                let is_active = idx == self.active_tab;
                                let is_playing_this = is_playing && self.playing_doc_idx == Some(idx);
                                let pf = if is_playing_this { cur_play_frame } else { None };
                                let act = draw_sound_forge_doc_window(
                                    ui,
                                    &mut self.docs[idx],
                                    is_active,
                                    false,
                                    has_multi,
                                    each_h,
                                    &mut self.interaction,
                                    pf,
                                    is_playing_this,
                                    self.loop_enabled,
                                    self.snap_zero_crossing,
                                    self.active_tool,
                                    self.skin,
                                );
                                if act != DocWindowAction::None {
                                    pending_action = Some((idx, act));
                                }
                                ui.add_space(4.0);
                            }
                        });
                    }

                    MdiLayoutMode::TileVertically => {
                        ui.columns(num_docs, |cols| {
                            for (idx, col_ui) in cols.iter_mut().enumerate() {
                                let is_active = idx == self.active_tab;
                                let is_playing_this = is_playing && self.playing_doc_idx == Some(idx);
                                let pf = if is_playing_this { cur_play_frame } else { None };
                                let act = draw_sound_forge_doc_window(
                                    col_ui,
                                    &mut self.docs[idx],
                                    is_active,
                                    false,
                                    has_multi,
                                    avail_h - 8.0,
                                    &mut self.interaction,
                                    pf,
                                    is_playing_this,
                                    self.loop_enabled,
                                    self.snap_zero_crossing,
                                    self.active_tool,
                                    self.skin,
                                );
                                if act != DocWindowAction::None {
                                    pending_action = Some((idx, act));
                                }
                            }
                        });
                    }

                    MdiLayoutMode::MaximizedTabs => {
                        ui.horizontal_wrapped(|ui| {
                            for (idx, d) in self.docs.iter().enumerate() {
                                if ui
                                    .selectable_label(
                                        idx == self.active_tab,
                                        d.formatted_title_with_duration(),
                                    )
                                    .clicked()
                                {
                                    self.active_tab = idx;
                                }
                            }
                        });
                        ui.add_space(2.0);
                        let idx = self.active_tab.min(num_docs - 1);
                        let is_playing_this = is_playing && self.playing_doc_idx == Some(idx);
                        let pf = if is_playing_this { cur_play_frame } else { None };
                        let act = draw_sound_forge_doc_window(
                            ui,
                            &mut self.docs[idx],
                            true,
                            true,
                            has_multi,
                            avail_h - 32.0,
                            &mut self.interaction,
                            pf,
                            is_playing_this,
                            self.loop_enabled,
                            self.snap_zero_crossing,
                            self.active_tool,
                            self.skin,
                        );
                        if act != DocWindowAction::None {
                            pending_action = Some((idx, act));
                        }
                    }
                }

                if let Some((idx, act)) = pending_action {
                    self.active_tab = idx;
                    match act {
                        DocWindowAction::Activate => {}
                        DocWindowAction::PlayFrom(frame) => {
                            if let Some(d) = self.docs.get_mut(idx) {
                                d.cursor_frame = frame;
                            }
                            self.play_doc(idx, true);
                        }
                        DocWindowAction::PlayWindow => {
                            if self.playback.is_playing() && self.playing_doc_idx == Some(idx) {
                                self.toggle_play();
                            } else {
                                self.play_doc(idx, false);
                            }
                        }
                        DocWindowAction::StopWindow => {
                            self.stop_playback();
                        }
                        DocWindowAction::ToggleLoopPlay => {
                            self.loop_enabled = !self.loop_enabled;
                            self.playback.set_loop(self.loop_enabled);
                            if !self.playback.is_playing() {
                                self.play_doc(idx, false);
                            }
                        }
                        DocWindowAction::ToggleMaximize => {
                            self.mdi_layout = if self.mdi_layout == MdiLayoutMode::MaximizedTabs {
                                MdiLayoutMode::TileHorizontally
                            } else {
                                MdiLayoutMode::MaximizedTabs
                            };
                        }
                        DocWindowAction::CloseWindow => {
                            self.stop_playback();
                            self.docs.remove(idx);
                            self.active_tab = self.active_tab.min(self.docs.len().saturating_sub(1));
                        }
                        DocWindowAction::SendSelectionToOtherDoc => {
                            self.dlg_target_doc_idx = (idx + 1) % self.docs.len().max(1);
                            self.dialog = ActiveDialog::SendToOtherDoc;
                        }
                        DocWindowAction::None => {}
                    }
                }
            });

        self.draw_dialogs(ctx);
    }
}

impl ForgeApp {
    fn apply_time_stretch_to_doc(&mut self, speed_factor: f64, algo: TimeStretchAlgorithm) {
        self.stop_playback();
        let Some(doc) = self.active_doc_mut() else {
            return;
        };
        let (s, e) = doc.target_range();
        let old_len = e - s;
        let next_buf = change_speed_range_with_algo(&doc.buffer, s, e, speed_factor, algo);
        let new_len = next_buf.num_frames() - (doc.buffer.num_frames() - old_len);
        doc.annotations.adjust_for_edit(s, e, new_len);
        let new_sel = doc.selection.map(|_| (s, s + new_len));
        doc.commit_buffer(next_buf, s, new_sel);
        let pct_orig = 100.0 / speed_factor;
        self.status_msg = format!(
            "Sony Time Stretch concluído: {:.2}% da duração original ({}).",
            pct_orig,
            algo.label()
        );
    }

    fn draw_dialog_time_boxes(&self, ui: &mut egui::Ui) {
        if let Some(doc) = self.active_doc() {
            let (s, e) = doc.target_range();
            let start_t = format_timecode(doc.buffer.frame_to_secs(s));
            let end_t = format_timecode(doc.buffer.frame_to_secs(e));
            let len_t = format_timecode(doc.buffer.frame_to_secs(e.saturating_sub(s)));
            ui.horizontal(|ui| {
                ui.label("Preview limit: 00:00:15,000");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(egui::RichText::new(format!("[ {len_t} ]")).monospace().size(11.0));
                    ui.label(egui::RichText::new(format!("[ {end_t} ]")).monospace().size(11.0));
                    ui.label(egui::RichText::new(format!("[ {start_t} ]")).monospace().size(11.0));
                });
            });
        }
    }

    fn draw_dialogs(&mut self, ctx: &egui::Context) {
        let doc_title = self
            .active_doc()
            .map(|d| d.title.clone())
            .unwrap_or_else(|| "Untitled".to_string());

        match self.dialog {
            ActiveDialog::None => {}

            // Exact Screenshot 3: `Volume - <Song Title>`
            ActiveDialog::Volume => {
                let mut open = true;
                egui::Window::new(format!("Volume - {doc_title}"))
                    .open(&mut open)
                    .resizable(false)
                    .show(ctx, |ui| {
                        ui.horizontal(|ui| {
                            ui.label("Preset:");
                            egui::ComboBox::from_id_salt("vol_preset")
                                .selected_text(&self.dlg_vol_preset)
                                .width(220.0)
                                .show_ui(ui, |ui| {
                                    for (name, db) in [
                                        ("(Untitled)", self.dlg_gain_db),
                                        ("+6.0 dB Boost (200%)", 6.0),
                                        ("+4.28 dB (163.68%)", 4.28),
                                        ("+3.0 dB Boost (141%)", 3.0),
                                        ("0.0 dB Unity (100%)", 0.0),
                                        ("-3.0 dB Cut (70.8%)", -3.0),
                                        ("-6.0 dB Cut (50.1%)", -6.0),
                                        ("-Inf. dB (Mute)", -96.0),
                                    ] {
                                        if ui
                                            .selectable_label(self.dlg_vol_preset == name, name)
                                            .clicked()
                                        {
                                            self.dlg_vol_preset = name.to_string();
                                            self.dlg_gain_db = db;
                                        }
                                    }
                                });
                            let _ = ui.button("💾");
                            let _ = ui.button("✖");
                        });

                        ui.add_space(6.0);
                        ui.horizontal(|ui| {
                            // Left bordered box with vertical Gain fader (Screenshot 3!)
                            egui::Frame::group(ui.style())
                                .show(ui, |ui| {
                                    ui.set_min_size(egui::vec2(280.0, 195.0));
                                    ui.vertical_centered(|ui| {
                                        let pct = if self.dlg_gain_db <= -90.0 {
                                            0.0
                                        } else {
                                            10.0_f32.powf(self.dlg_gain_db / 20.0) * 100.0
                                        };
                                        let db_lbl = if self.dlg_gain_db <= -90.0 {
                                            "-Inf. dB".to_string()
                                        } else {
                                            format!("{:.2} dB", self.dlg_gain_db).replace('.', ",")
                                        };
                                        let pct_lbl =
                                            format!("({:.2} %)", pct).replace('.', ",");
                                        ui.label(egui::RichText::new(db_lbl).strong().size(13.0));
                                        ui.label(egui::RichText::new(pct_lbl).size(12.5));
                                        ui.add_space(4.0);
                                        ui.add(
                                            egui::Slider::new(&mut self.dlg_gain_db, -60.0..=20.0)
                                                .vertical()
                                                .show_value(false),
                                        );
                                        ui.add_space(4.0);
                                        ui.label("Gain");
                                        ui.label("(-Inf. to 20 dB)");
                                    });
                                });

                            // Right button column (OK, Cancel, Preview, Bypass, More...)
                            ui.vertical(|ui| {
                                if ui
                                    .add_sized([92.0, 26.0], egui::Button::new("OK"))
                                    .clicked()
                                {
                                    self.stop_playback();
                                    let db = self.dlg_gain_db;
                                    if let Some(doc) = self.active_doc_mut() {
                                        let (s, e) = doc.target_range();
                                        let next = apply_gain_db(&doc.buffer, s, e, db);
                                        doc.commit_buffer(next, doc.cursor_frame, doc.selection);
                                    }
                                    self.status_msg = format!("Volume ({db:+.2} dB) aplicado.");
                                    self.dialog = ActiveDialog::None;
                                }
                                if ui
                                    .add_sized([92.0, 26.0], egui::Button::new("Cancel"))
                                    .clicked()
                                {
                                    self.stop_playback();
                                    self.dialog = ActiveDialog::None;
                                }
                                ui.add_space(6.0);
                                if ui
                                    .add_sized([92.0, 26.0], egui::Button::new("Preview"))
                                    .clicked()
                                {
                                    if let Some(doc) = self.active_doc() {
                                        let (s, e) = doc.target_range();
                                        let seg = slice_frames(&doc.buffer, s, e);
                                        let db = if self.dlg_bypass {
                                            0.0
                                        } else {
                                            self.dlg_gain_db
                                        };
                                        let proc = apply_gain_db(&seg, 0, seg.num_frames(), db);
                                        self.preview_processed_buffer(proc);
                                    }
                                }
                                ui.checkbox(&mut self.dlg_bypass, "Bypass");
                                ui.add_space(42.0);
                                let _ = ui.add_sized([92.0, 24.0], egui::Button::new("More..."));
                            });
                        });

                        ui.separator();
                        self.draw_dialog_time_boxes(ui);
                    });
                if !open {
                    self.dialog = ActiveDialog::None;
                }
            }

            // Exact Screenshot 4: `Sony Time Stretch - <Song Title>`
            ActiveDialog::TimeStretch => {
                let mut open = true;
                let init_secs = self.active_target_duration_secs();
                egui::Window::new(format!("Sony Time Stretch - {doc_title}"))
                    .open(&mut open)
                    .resizable(false)
                    .show(ctx, |ui| {
                        ui.horizontal(|ui| {
                            ui.label("Preset:");
                            egui::ComboBox::from_id_salt("ts_preset")
                                .selected_text(&self.dlg_ts_preset)
                                .width(270.0)
                                .show_ui(ui, |ui| {
                                    for (name, pct_orig) in [
                                        ("(Untitled)", self.dlg_ts_percent_of_orig),
                                        ("80% of original time (25% mais rápida)", 80.0),
                                        ("89,85% of original time (Mais rápida)", 89.85),
                                        ("95% of original time (Levemente mais rápida)", 95.0),
                                        ("100% Original", 100.0),
                                        ("105% of original time (Levemente mais lenta)", 105.0),
                                        ("110% of original time (Mais lenta p/ ensaio)", 110.0),
                                        ("125% of original time (Bem mais lenta)", 125.0),
                                    ] {
                                        if ui
                                            .selectable_label(self.dlg_ts_preset == name, name)
                                            .clicked()
                                        {
                                            self.dlg_ts_preset = name.to_string();
                                            self.dlg_ts_percent_of_orig = pct_orig;
                                        }
                                    }
                                });
                            let _ = ui.button("💾");
                            let _ = ui.button("✖");
                        });

                        ui.add_space(4.0);
                        ui.horizontal(|ui| {
                            egui::Frame::group(ui.style()).show(ui, |ui| {
                                ui.set_min_width(370.0);
                                // Classic SONY | Time Stretch dark banner (Screenshot 4!)
                                egui::Frame::none()
                                    .fill(egui::Color32::from_rgb(128, 128, 128))
                                    .inner_margin(egui::Margin::symmetric(8.0, 4.0))
                                    .show(ui, |ui| {
                                        ui.horizontal(|ui| {
                                            ui.label(
                                                egui::RichText::new("SONY.")
                                                    .strong()
                                                    .size(14.0)
                                                    .color(egui::Color32::BLACK),
                                            );
                                            ui.separator();
                                            ui.label(
                                                egui::RichText::new("Time Stretch")
                                                    .strong()
                                                    .size(13.0)
                                                    .color(egui::Color32::WHITE),
                                            );
                                        });
                                    });

                                ui.add_space(6.0);
                                ui.horizontal(|ui| {
                                    ui.label("Mode:");
                                    egui::ComboBox::from_id_salt("ts_mode_combo")
                                        .selected_text(self.dlg_ts_algo.label())
                                        .width(290.0)
                                        .show_ui(ui, |ui| {
                                            for &algo in TimeStretchAlgorithm::all() {
                                                ui.selectable_value(
                                                    &mut self.dlg_ts_algo,
                                                    algo,
                                                    algo.label(),
                                                );
                                            }
                                        });
                                });

                                let min_s = init_secs * 0.5;
                                let max_s = init_secs * 5.0;
                                let final_secs = init_secs * (self.dlg_ts_percent_of_orig / 100.0);
                                ui.add_space(4.0);
                                ui.horizontal(|ui| {
                                    ui.label(format!(
                                        "Final Time ({} to {}):",
                                        format_timecode(min_s),
                                        format_timecode(max_s)
                                    ));
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            ui.label(
                                                egui::RichText::new(format!(
                                                    "[ {} ]",
                                                    format_timecode(final_secs)
                                                ))
                                                .monospace()
                                                .strong(),
                                            );
                                        },
                                    );
                                });

                                ui.add(
                                    egui::Slider::new(
                                        &mut self.dlg_ts_percent_of_orig,
                                        50.0..=250.0,
                                    )
                                    .show_value(false),
                                );

                                ui.separator();
                                ui.horizontal(|ui| {
                                    ui.label("Input format:");
                                    egui::ComboBox::from_id_salt("ts_input_fmt")
                                        .selected_text(self.dlg_ts_format.label())
                                        .width(180.0)
                                        .show_ui(ui, |ui| {
                                            for fmt in [
                                                TimeStretchInputFormat::TimeHms,
                                                TimeStretchInputFormat::Percent,
                                                TimeStretchInputFormat::TempoBpm,
                                            ] {
                                                ui.selectable_value(
                                                    &mut self.dlg_ts_format,
                                                    fmt,
                                                    fmt.label(),
                                                );
                                            }
                                        });
                                });

                                if self.dlg_ts_format == TimeStretchInputFormat::TempoBpm {
                                    ui.horizontal(|ui| {
                                        ui.label("BPM Inicial:");
                                        ui.add(
                                            egui::DragValue::new(&mut self.dlg_ts_bpm_from)
                                                .range(30.0..=300.0),
                                        );
                                        ui.label("BPM Final:");
                                        if ui
                                            .add(
                                                egui::DragValue::new(&mut self.dlg_ts_bpm_to)
                                                    .range(30.0..=300.0),
                                            )
                                            .changed()
                                            && self.dlg_ts_bpm_to > 0.0
                                        {
                                            self.dlg_ts_percent_of_orig = ((self.dlg_ts_bpm_from
                                                / self.dlg_ts_bpm_to)
                                                * 100.0)
                                                .clamp(50.0, 250.0);
                                        }
                                    });
                                }

                                ui.horizontal(|ui| {
                                    ui.label("Initial Time (hr:mn:sc.xxx):");
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            ui.label(
                                                egui::RichText::new(format!(
                                                    "[ {} ]",
                                                    format_timecode(init_secs)
                                                ))
                                                .monospace(),
                                            );
                                        },
                                    );
                                });

                                ui.horizontal(|ui| {
                                    ui.label("Percent of original:");
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            let speed_hint = if self.dlg_ts_percent_of_orig < 99.8 {
                                                " (Acelerada)"
                                            } else if self.dlg_ts_percent_of_orig > 100.2 {
                                                " (Ralentada)"
                                            } else {
                                                ""
                                            };
                                            ui.label(
                                                egui::RichText::new(format!(
                                                    "{:.2} %{speed_hint}",
                                                    self.dlg_ts_percent_of_orig
                                                ).replace('.', ","))
                                                .strong(),
                                            );
                                        },
                                    );
                                });
                            });

                            // Right column buttons (OK, Cancel, Preview, Bypass, Real-time, More...)
                            ui.vertical(|ui| {
                                let speed_factor =
                                    (100.0 / self.dlg_ts_percent_of_orig.max(25.0)).clamp(0.25, 4.0);
                                if ui
                                    .add_sized([92.0, 26.0], egui::Button::new("OK"))
                                    .clicked()
                                {
                                    let algo = self.dlg_ts_algo;
                                    self.apply_time_stretch_to_doc(speed_factor, algo);
                                    self.dialog = ActiveDialog::None;
                                }
                                if ui
                                    .add_sized([92.0, 26.0], egui::Button::new("Cancel"))
                                    .clicked()
                                {
                                    self.stop_playback();
                                    self.dialog = ActiveDialog::None;
                                }
                                ui.add_space(6.0);
                                if ui
                                    .add_sized([92.0, 26.0], egui::Button::new("Preview"))
                                    .clicked()
                                {
                                    if let Some(doc) = self.active_doc() {
                                        let (s, e) = doc.target_range();
                                        let seg = slice_frames(&doc.buffer, s, e);
                                        let spd = if self.dlg_bypass { 1.0 } else { speed_factor };
                                        let proc = change_speed_range_with_algo(
                                            &seg,
                                            0,
                                            seg.num_frames(),
                                            spd,
                                            self.dlg_ts_algo,
                                        );
                                        self.preview_processed_buffer(proc);
                                    }
                                }
                                ui.checkbox(&mut self.dlg_bypass, "Bypass");
                                ui.checkbox(&mut self.dlg_realtime, "Real-time");
                                ui.add_space(32.0);
                                let _ = ui.add_sized([92.0, 24.0], egui::Button::new("More..."));
                            });
                        });

                        ui.separator();
                        self.draw_dialog_time_boxes(ui);
                    });
                if !open {
                    self.dialog = ActiveDialog::None;
                }
            }

            // Graphic Fade Dialog (`Process -> Fade -> Graphic...`)
            ActiveDialog::GraphicFade => {
                let mut open = true;
                egui::Window::new(format!("Graphic Fade - {doc_title}"))
                    .open(&mut open)
                    .resizable(false)
                    .show(ctx, |ui| {
                        ui.horizontal(|ui| {
                            ui.label("Início (%):");
                            ui.add(
                                egui::Slider::new(&mut self.dlg_fade_start_pct, 0.0..=200.0)
                                    .suffix("%"),
                            );
                        });
                        ui.horizontal(|ui| {
                            ui.label("Fim (%):");
                            ui.add(
                                egui::Slider::new(&mut self.dlg_fade_end_pct, 0.0..=200.0)
                                    .suffix("%"),
                            );
                        });
                        ui.horizontal(|ui| {
                            if ui.button("Preset: Fade In (0% -> 100%)").clicked() {
                                self.dlg_fade_start_pct = 0.0;
                                self.dlg_fade_end_pct = 100.0;
                            }
                            if ui.button("Preset: Fade Out (100% -> 0%)").clicked() {
                                self.dlg_fade_start_pct = 100.0;
                                self.dlg_fade_end_pct = 0.0;
                            }
                        });
                        for curve in [
                            FadeCurve::Linear,
                            FadeCurve::SmoothSCurve,
                            FadeCurve::Exponential,
                            FadeCurve::Logarithmic,
                        ] {
                            ui.radio_value(&mut self.dlg_fade_curve, curve, curve.label());
                        }
                        ui.separator();
                        ui.horizontal(|ui| {
                            if ui.button("OK").clicked() {
                                self.stop_playback();
                                let g0 = self.dlg_fade_start_pct / 100.0;
                                let g1 = self.dlg_fade_end_pct / 100.0;
                                let c = self.dlg_fade_curve;
                                if let Some(doc) = self.active_doc_mut() {
                                    let (s, e) = doc.target_range();
                                    let next = graphic_fade(&doc.buffer, s, e, g0, g1, c);
                                    doc.commit_buffer(next, doc.cursor_frame, doc.selection);
                                }
                                self.dialog = ActiveDialog::None;
                            }
                            if ui.button("Preview").clicked() {
                                if let Some(doc) = self.active_doc() {
                                    let (s, e) = doc.target_range();
                                    let seg = slice_frames(&doc.buffer, s, e);
                                    let proc = graphic_fade(
                                        &seg,
                                        0,
                                        seg.num_frames(),
                                        self.dlg_fade_start_pct / 100.0,
                                        self.dlg_fade_end_pct / 100.0,
                                        self.dlg_fade_curve,
                                    );
                                    self.preview_processed_buffer(proc);
                                }
                            }
                            if ui.button("Cancel").clicked() {
                                self.stop_playback();
                                self.dialog = ActiveDialog::None;
                            }
                        });
                    });
                if !open {
                    self.dialog = ActiveDialog::None;
                }
            }

            // Normalize Dialog (`Process -> Normalize...`)
            ActiveDialog::Normalize => {
                let mut open = true;
                egui::Window::new(format!("Normalize - {doc_title}"))
                    .open(&mut open)
                    .resizable(false)
                    .show(ctx, |ui| {
                        ui.radio_value(&mut self.dlg_norm_rms, false, "Peak level (Nível de Pico)");
                        ui.radio_value(
                            &mut self.dlg_norm_rms,
                            true,
                            "Average RMS level (Volume Percebido)",
                        );
                        ui.horizontal(|ui| {
                            ui.label("Normalize to:");
                            ui.add(
                                egui::Slider::new(&mut self.dlg_norm_db, -18.0..=0.0).suffix(" dB"),
                            );
                        });
                        ui.separator();
                        ui.horizontal(|ui| {
                            if ui.button("OK").clicked() {
                                self.stop_playback();
                                let db = self.dlg_norm_db;
                                let rms = self.dlg_norm_rms;
                                if let Some(doc) = self.active_doc_mut() {
                                    let (s, e) = doc.target_range();
                                    let next = normalize_audio(&doc.buffer, s, e, db, rms);
                                    doc.commit_buffer(next, doc.cursor_frame, doc.selection);
                                }
                                self.dialog = ActiveDialog::None;
                            }
                            if ui.button("Cancel").clicked() {
                                self.dialog = ActiveDialog::None;
                            }
                        });
                    });
                if !open {
                    self.dialog = ActiveDialog::None;
                }
            }

            // Auto Trim/Crop (`Process -> Auto Trim/Crop...`)
            ActiveDialog::AutoTrimCrop => {
                let mut open = true;
                egui::Window::new(format!("Auto Trim/Crop - {doc_title}"))
                    .open(&mut open)
                    .resizable(false)
                    .show(ctx, |ui| {
                        ui.label("Remove automaticamente o silêncio inicial e final abaixo do limiar:");
                        ui.horizontal(|ui| {
                            ui.label("Threshold (Limiar):");
                            ui.add(
                                egui::Slider::new(&mut self.dlg_trim_thresh_db, -72.0..=-12.0)
                                    .suffix(" dB"),
                            );
                        });
                        ui.horizontal(|ui| {
                            if ui.button("OK").clicked() {
                                self.stop_playback();
                                let th = self.dlg_trim_thresh_db;
                                if let Some(doc) = self.active_doc_mut() {
                                    let (s, e) = doc.target_range();
                                    let next = auto_trim_crop(&doc.buffer, s, e, th);
                                    doc.commit_buffer(next, 0, None);
                                    doc.zoom_fit();
                                }
                                self.status_msg = "Auto Trim/Crop aplicado.".to_string();
                                self.dialog = ActiveDialog::None;
                            }
                            if ui.button("Cancel").clicked() {
                                self.dialog = ActiveDialog::None;
                            }
                        });
                    });
                if !open {
                    self.dialog = ActiveDialog::None;
                }
            }

            // Channel Converter (`Process -> Channel Converter...`)
            ActiveDialog::ChannelConverter => {
                let mut open = true;
                egui::Window::new(format!("Channel Converter - {doc_title}"))
                    .open(&mut open)
                    .resizable(false)
                    .show(ctx, |ui| {
                        for &m in ChannelConvertMode::all() {
                            ui.radio_value(&mut self.dlg_chan_mode, m, m.label());
                        }
                        ui.separator();
                        ui.horizontal(|ui| {
                            if ui.button("OK").clicked() {
                                self.stop_playback();
                                let mode = self.dlg_chan_mode;
                                if let Some(doc) = self.active_doc_mut() {
                                    let next = convert_channels(&doc.buffer, mode);
                                    doc.commit_buffer(next, doc.cursor_frame, doc.selection);
                                }
                                self.dialog = ActiveDialog::None;
                            }
                            if ui.button("Cancel").clicked() {
                                self.dialog = ActiveDialog::None;
                            }
                        });
                    });
                if !open {
                    self.dialog = ActiveDialog::None;
                }
            }

            // Pan / Expand (`Process -> Pan/Expand...`)
            ActiveDialog::PanExpand => {
                let mut open = true;
                egui::Window::new(format!("Pan/Expand - {doc_title}"))
                    .open(&mut open)
                    .resizable(false)
                    .show(ctx, |ui| {
                        ui.horizontal(|ui| {
                            ui.label("Pan (Left -1.0 .. Right +1.0):");
                            ui.add(egui::Slider::new(&mut self.dlg_pan, -1.0..=1.0));
                        });
                        ui.horizontal(|ui| {
                            ui.label("Stereo Width (0=Mono, 1=Normal, 2=Wide):");
                            ui.add(egui::Slider::new(&mut self.dlg_stereo_width, 0.0..=2.0));
                        });
                        ui.horizontal(|ui| {
                            if ui.button("OK").clicked() {
                                self.stop_playback();
                                let pan = self.dlg_pan;
                                let w = self.dlg_stereo_width;
                                if let Some(doc) = self.active_doc_mut() {
                                    let (s, e) = doc.target_range();
                                    let next = pan_expand_range(&doc.buffer, s, e, pan, w);
                                    doc.commit_buffer(next, doc.cursor_frame, doc.selection);
                                }
                                self.dialog = ActiveDialog::None;
                            }
                            if ui.button("Cancel").clicked() {
                                self.dialog = ActiveDialog::None;
                            }
                        });
                    });
                if !open {
                    self.dialog = ActiveDialog::None;
                }
            }

            // Smooth / Enhance (`Process -> Smooth/Enhance...`)
            ActiveDialog::SmoothEnhance => {
                let mut open = true;
                egui::Window::new(format!("Smooth/Enhance - {doc_title}"))
                    .open(&mut open)
                    .resizable(false)
                    .show(ctx, |ui| {
                        ui.label("Operation (-5 = Smooth Clicks .. +5 = Enhance Brilliance):");
                        ui.add(egui::Slider::new(&mut self.dlg_smooth_amount, -5..=5));
                        ui.horizontal(|ui| {
                            if ui.button("OK").clicked() {
                                self.stop_playback();
                                let amt = self.dlg_smooth_amount;
                                if let Some(doc) = self.active_doc_mut() {
                                    let (s, e) = doc.target_range();
                                    let next = smooth_enhance_range(&doc.buffer, s, e, amt);
                                    doc.commit_buffer(next, doc.cursor_frame, doc.selection);
                                }
                                self.dialog = ActiveDialog::None;
                            }
                            if ui.button("Cancel").clicked() {
                                self.dialog = ActiveDialog::None;
                            }
                        });
                    });
                if !open {
                    self.dialog = ActiveDialog::None;
                }
            }

            // 3-Band EQ (`Process -> EQ`)
            ActiveDialog::Eq3Band => {
                let mut open = true;
                egui::Window::new(format!("Graphic / Paragraphic EQ - {doc_title}"))
                    .open(&mut open)
                    .resizable(false)
                    .show(ctx, |ui| {
                        ui.horizontal(|ui| {
                            ui.label("Low (< 250 Hz):");
                            ui.add(
                                egui::Slider::new(&mut self.dlg_eq_low_db, -18.0..=18.0)
                                    .suffix(" dB"),
                            );
                        });
                        ui.horizontal(|ui| {
                            ui.label("Mid (~ 1 kHz):");
                            ui.add(
                                egui::Slider::new(&mut self.dlg_eq_mid_db, -18.0..=18.0)
                                    .suffix(" dB"),
                            );
                        });
                        ui.horizontal(|ui| {
                            ui.label("High (> 3.5 kHz):");
                            ui.add(
                                egui::Slider::new(&mut self.dlg_eq_high_db, -18.0..=18.0)
                                    .suffix(" dB"),
                            );
                        });
                        ui.horizontal(|ui| {
                            if ui.button("OK").clicked() {
                                self.stop_playback();
                                let (l, m, h) =
                                    (self.dlg_eq_low_db, self.dlg_eq_mid_db, self.dlg_eq_high_db);
                                if let Some(doc) = self.active_doc_mut() {
                                    let (s, e) = doc.target_range();
                                    let next = apply_3band_eq(&doc.buffer, s, e, l, m, h);
                                    doc.commit_buffer(next, doc.cursor_frame, doc.selection);
                                }
                                self.dialog = ActiveDialog::None;
                            }
                            if ui.button("Preview").clicked() {
                                if let Some(doc) = self.active_doc() {
                                    let (s, e) = doc.target_range();
                                    let seg = slice_frames(&doc.buffer, s, e);
                                    let proc = apply_3band_eq(
                                        &seg,
                                        0,
                                        seg.num_frames(),
                                        self.dlg_eq_low_db,
                                        self.dlg_eq_mid_db,
                                        self.dlg_eq_high_db,
                                    );
                                    self.preview_processed_buffer(proc);
                                }
                            }
                            if ui.button("Cancel").clicked() {
                                self.stop_playback();
                                self.dialog = ActiveDialog::None;
                            }
                        });
                    });
                if !open {
                    self.dialog = ActiveDialog::None;
                }
            }

            // Pitch Shift (`Effects -> Pitch -> Shift...`)
            ActiveDialog::PitchShift => {
                let mut open = true;
                egui::Window::new(format!("Pitch Shift - {doc_title}"))
                    .open(&mut open)
                    .resizable(false)
                    .show(ctx, |ui| {
                        ui.horizontal(|ui| {
                            ui.label("Semitones (-12 to +12):");
                            ui.add(egui::Slider::new(&mut self.dlg_pitch_semitones, -12.0..=12.0));
                        });
                        ui.horizontal(|ui| {
                            ui.label("Cents (-50 to +50):");
                            ui.add(egui::Slider::new(&mut self.dlg_pitch_cents, -50.0..=50.0));
                        });
                        ui.checkbox(
                            &mut self.dlg_pitch_preserve_dur,
                            "Preserve duration (manter duração original)",
                        );
                        ui.horizontal(|ui| {
                            if ui.button("OK").clicked() {
                                self.stop_playback();
                                let total_st =
                                    self.dlg_pitch_semitones + self.dlg_pitch_cents / 100.0;
                                let pres = self.dlg_pitch_preserve_dur;
                                if let Some(doc) = self.active_doc_mut() {
                                    let (s, e) = doc.target_range();
                                    let next =
                                        pitch_shift_range_ex(&doc.buffer, s, e, total_st, pres);
                                    doc.commit_buffer(next, doc.cursor_frame, None);
                                }
                                self.dialog = ActiveDialog::None;
                            }
                            if ui.button("Cancel").clicked() {
                                self.dialog = ActiveDialog::None;
                            }
                        });
                    });
                if !open {
                    self.dialog = ActiveDialog::None;
                }
            }

            // Delay/Echo (`Effects -> Delay/Echo...`)
            ActiveDialog::DelayEcho => {
                let mut open = true;
                egui::Window::new(format!("Simple Delay/Echo - {doc_title}"))
                    .open(&mut open)
                    .resizable(false)
                    .show(ctx, |ui| {
                        ui.horizontal(|ui| {
                            ui.label("Delay Time (ms):");
                            ui.add(
                                egui::Slider::new(&mut self.dlg_delay_ms, 20.0..=1000.0)
                                    .suffix(" ms"),
                            );
                        });
                        ui.horizontal(|ui| {
                            ui.label("Feedback:");
                            ui.add(egui::Slider::new(&mut self.dlg_delay_fb, 0.0..=0.85));
                        });
                        ui.horizontal(|ui| {
                            ui.label("Wet Mix:");
                            ui.add(egui::Slider::new(&mut self.dlg_delay_wet, 0.0..=1.0));
                        });
                        ui.horizontal(|ui| {
                            if ui.button("OK").clicked() {
                                self.stop_playback();
                                let (ms, fb, w) =
                                    (self.dlg_delay_ms, self.dlg_delay_fb, self.dlg_delay_wet);
                                if let Some(doc) = self.active_doc_mut() {
                                    let (s, e) = doc.target_range();
                                    let next = apply_delay_echo(&doc.buffer, s, e, ms, fb, w);
                                    doc.commit_buffer(next, doc.cursor_frame, doc.selection);
                                }
                                self.dialog = ActiveDialog::None;
                            }
                            if ui.button("Cancel").clicked() {
                                self.dialog = ActiveDialog::None;
                            }
                        });
                    });
                if !open {
                    self.dialog = ActiveDialog::None;
                }
            }

            // Reverb (`Effects -> Reverb...`)
            ActiveDialog::Reverb => {
                let mut open = true;
                egui::Window::new(format!("Reverb - {doc_title}"))
                    .open(&mut open)
                    .resizable(false)
                    .show(ctx, |ui| {
                        ui.horizontal(|ui| {
                            ui.label("Room Size:");
                            ui.add(egui::Slider::new(&mut self.dlg_rev_room, 0.1..=0.98));
                        });
                        ui.horizontal(|ui| {
                            ui.label("High-Freq Damping:");
                            ui.add(egui::Slider::new(&mut self.dlg_rev_damp, 0.05..=0.90));
                        });
                        ui.horizontal(|ui| {
                            ui.label("Wet Level:");
                            ui.add(egui::Slider::new(&mut self.dlg_rev_wet, 0.05..=0.80));
                        });
                        ui.horizontal(|ui| {
                            if ui.button("OK").clicked() {
                                self.stop_playback();
                                let (r, d, w) =
                                    (self.dlg_rev_room, self.dlg_rev_damp, self.dlg_rev_wet);
                                if let Some(doc) = self.active_doc_mut() {
                                    let (s, e) = doc.target_range();
                                    let next = apply_reverb(&doc.buffer, s, e, r, d, w);
                                    doc.commit_buffer(next, doc.cursor_frame, doc.selection);
                                }
                                self.dialog = ActiveDialog::None;
                            }
                            if ui.button("Cancel").clicked() {
                                self.dialog = ActiveDialog::None;
                            }
                        });
                    });
                if !open {
                    self.dialog = ActiveDialog::None;
                }
            }

            // Send Selection to Another Open Song (`Como colocar varias musicas funciona`)
            ActiveDialog::SendToOtherDoc => {
                let mut open = true;
                egui::Window::new("Transferir / Colar Trecho em Outra Música Aberta")
                    .open(&mut open)
                    .resizable(false)
                    .show(ctx, |ui| {
                        if self.docs.len() < 2 {
                            ui.label("Abra pelo menos 2 músicas (Ctrl+O) para transferir trechos entre elas.");
                            return;
                        }
                        ui.label(format!("Origem: {}", doc_title));
                        ui.horizontal(|ui| {
                            ui.label("Música de Destino:");
                            let dst_idx = self.dlg_target_doc_idx.min(self.docs.len() - 1);
                            egui::ComboBox::from_id_salt("dst_doc_combo")
                                .selected_text(&self.docs[dst_idx].title)
                                .show_ui(ui, |ui| {
                                    for (i, d) in self.docs.iter().enumerate() {
                                        if i != self.active_tab {
                                            ui.selectable_value(
                                                &mut self.dlg_target_doc_idx,
                                                i,
                                                format!("{}. {}", i + 1, d.title),
                                            );
                                        }
                                    }
                                });
                        });

                        ui.separator();
                        ui.radio_value(
                            &mut self.dlg_transfer_mode,
                            0,
                            "Inserir exatamente na posição do cursor da música de destino",
                        );
                        ui.radio_value(
                            &mut self.dlg_transfer_mode,
                            1,
                            "Juntar no final da música de destino com Crossfade suave",
                        );
                        ui.radio_value(
                            &mut self.dlg_transfer_mode,
                            2,
                            "Mixar (sobrepor) na posição do cursor da música de destino",
                        );

                        if self.dlg_transfer_mode == 1 {
                            ui.horizontal(|ui| {
                                ui.label("Crossfade (s):");
                                ui.add(
                                    egui::DragValue::new(&mut self.dlg_join_xfade_secs)
                                        .speed(0.1)
                                        .range(0.0..=15.0)
                                        .suffix(" s"),
                                );
                            });
                        }

                        ui.add_space(6.0);
                        ui.horizontal(|ui| {
                            if ui.button("✔ Enviar Trecho Agora").clicked() {
                                self.stop_playback();
                                let src_idx = self.active_tab;
                                let dst_idx = self.dlg_target_doc_idx.min(self.docs.len() - 1);
                                if src_idx != dst_idx {
                                    let (s, e) = self.docs[src_idx].target_range();
                                    let clip = slice_frames(&self.docs[src_idx].buffer, s, e);
                                    let dst_doc = &mut self.docs[dst_idx];
                                    let at = dst_doc.cursor_frame;
                                    let next_buf = match self.dlg_transfer_mode {
                                        0 => insert_buffer(&dst_doc.buffer, at, &clip),
                                        1 => concat_with_crossfade(
                                            &dst_doc.buffer,
                                            &clip,
                                            self.dlg_join_xfade_secs,
                                        ),
                                        _ => mix_buffer(&dst_doc.buffer, at, &clip, 0.85),
                                    };
                                    dst_doc.commit_buffer(next_buf, at, None);
                                    self.active_tab = dst_idx;
                                    self.status_msg =
                                        "Trecho inserido na música de destino!".to_string();
                                }
                                self.dialog = ActiveDialog::None;
                            }
                            if ui.button("Cancel").clicked() {
                                self.dialog = ActiveDialog::None;
                            }
                        });
                    });
                if !open {
                    self.dialog = ActiveDialog::None;
                }
            }

            // Join / Merge All Open Songs (`Tools -> Join / Merge Open Songs...`)
            ActiveDialog::JoinOpenSongs => {
                let mut open = true;
                egui::Window::new("Juntar / Concatenar Músicas Abertas")
                    .open(&mut open)
                    .resizable(false)
                    .show(ctx, |ui| {
                        ui.label("Combina todas as músicas abertas (ou as seleções de cada uma) em uma nova faixa contínua:");
                        ui.add_space(4.0);
                        for (i, d) in self.docs.iter().enumerate() {
                            let (s, e) = d.target_range();
                            let dur = format_timecode(d.buffer.frame_to_secs(e - s));
                            ui.label(format!(
                                "{}. {} — Trecho: {} {}",
                                i + 1,
                                d.title,
                                dur,
                                if d.selection.is_some() {
                                    "(Seleção)"
                                } else {
                                    "(Música inteira)"
                                }
                            ));
                        }
                        ui.separator();
                        ui.horizontal(|ui| {
                            ui.label("Crossfade entre músicas (negativo = pausa de silêncio):");
                            ui.add(
                                egui::DragValue::new(&mut self.dlg_join_xfade_secs)
                                    .speed(0.1)
                                    .range(-10.0..=15.0)
                                    .suffix(" s"),
                            );
                        });
                        ui.add_space(6.0);
                        ui.horizontal(|ui| {
                            if ui.button("✔ Criar Música Mesclada").clicked()
                                && !self.docs.is_empty()
                            {
                                self.stop_playback();
                                let (s0, e0) = self.docs[0].target_range();
                                let mut acc = slice_frames(&self.docs[0].buffer, s0, e0);
                                for d in self.docs.iter().skip(1) {
                                    let (s, e) = d.target_range();
                                    let seg = slice_frames(&d.buffer, s, e);
                                    acc = concat_with_crossfade(&acc, &seg, self.dlg_join_xfade_secs);
                                }
                                let id = self.next_doc_id;
                                self.next_doc_id += 1;
                                let merged_doc = AudioDocument::from_buffer(
                                    id,
                                    format!("Mixagem_{id}.wav"),
                                    None,
                                    acc,
                                );
                                self.docs.push(merged_doc);
                                self.active_tab = self.docs.len() - 1;
                                self.status_msg =
                                    "Nova janela criada com as músicas unidas!".to_string();
                                self.dialog = ActiveDialog::None;
                            }
                            if ui.button("Cancel").clicked() {
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
                                    let next = extend_range_with_crossfade(
                                        &doc.buffer,
                                        s,
                                        e,
                                        secs,
                                        xfade,
                                    );
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

            ActiveDialog::InsertSilence => {
                let mut open = true;
                egui::Window::new("Insert Silence")
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
                        if ui.button("OK").clicked() {
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
                egui::Window::new("Save As / Render As")
                    .open(&mut open)
                    .resizable(false)
                    .show(ctx, |ui| {
                        ui.label("Save as type:");
                        for &fmt in ExportFormat::all() {
                            let enabled = !fmt.requires_ffmpeg() || self.ffmpeg_path.is_some();
                            ui.add_enabled_ui(enabled, |ui| {
                                ui.radio_value(&mut self.dlg_export_format, fmt, fmt.label());
                            });
                        }
                        ui.separator();
                        ui.horizontal(|ui| {
                            ui.label("Sample Rate:");
                            ui.radio_value(&mut self.dlg_export_sr, None, "Original");
                            ui.radio_value(&mut self.dlg_export_sr, Some(44100), "44,100 Hz");
                            ui.radio_value(&mut self.dlg_export_sr, Some(48000), "48,000 Hz");
                        });
                        ui.add_space(6.0);
                        ui.horizontal(|ui| {
                            if ui.button("📤 Save...").clicked() {
                                self.run_export();
                            }
                            if ui.button("Cancel").clicked() {
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
                egui::Window::new("Atalhos de Teclado (Sound Forge Pro 10.0)")
                    .open(&mut open)
                    .resizable(false)
                    .show(ctx, |ui| {
                        let rows = [
                            ("Espaço", "Play / Pause na janela ativa"),
                            ("Enter", "Parar e retornar cursor"),
                            ("M / R", "Inserir Marcador / Criar Região"),
                            ("L / Z", "Alternar Loop / Snap Zero-Crossing"),
                            ("Duplo Clique", "Selecionar entre marcadores (ou tudo)"),
                            ("Ctrl+C / Ctrl+V", "Copiar de uma música e colar em outra"),
                            ("Ctrl+M", "Paste Special -> Mix (sobrepor músicas)"),
                            ("Ctrl+T", "Trim / Crop para a seleção"),
                            ("Ctrl+Z / Ctrl+Y", "Undo / Redo"),
                            ("Ctrl+0", "Mostrar música inteira (Fit All)"),
                        ];
                        egui::Grid::new("sf10_shortcuts").striped(true).show(ui, |ui| {
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
                egui::Window::new("About — Sound Forge Linux")
                    .open(&mut open)
                    .resizable(false)
                    .show(ctx, |ui| {
                        ui.heading("Sound Forge Linux v0.2.0 (Pro 10.0 Layout)");
                        ui.label("Reconstrução clean-room em Rust nativo fiel ao fluxo de trabalho do Sony Sound Forge Pro 10.0.");
                        ui.add_space(4.0);
                        ui.label("• Bancada MDI Multi-Músicas (Empilhadas, Lado a Lado ou Abas)");
                        ui.label("• Diálogos fiéis: Sony Time Stretch (A01..A05 + Final Time) & Volume (-Inf..+20 dB)");
                        ui.label("• Menu Process completo: Auto Trim/Crop, Channel Converter, DC Offset, EQ, Fade, Normalize, Pan/Expand, Resample, Reverse, Rotate, Smooth/Enhance");
                    });
                if !open {
                    self.dialog = ActiveDialog::None;
                }
            }
        }
    }
}
