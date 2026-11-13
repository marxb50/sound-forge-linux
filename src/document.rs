use crate::audio::{AudioBuffer, WaveformPyramid};
use crate::markers::AnnotationSet;
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Clone)]
pub struct UndoSnapshot {
    pub buffer: Arc<AudioBuffer>,
    pub annotations: AnnotationSet,
    pub cursor_frame: usize,
    pub selection: Option<(usize, usize)>,
}

pub struct AudioDocument {
    pub id: usize,
    pub title: String,
    pub path: Option<PathBuf>,
    pub buffer: Arc<AudioBuffer>,
    pub pyramid: WaveformPyramid,
    pub annotations: AnnotationSet,
    pub undo_stack: Vec<UndoSnapshot>,
    pub redo_stack: Vec<UndoSnapshot>,
    pub cursor_frame: usize,
    pub selection: Option<(usize, usize)>,
    pub view_start: f64,
    pub view_frames: f64,
    pub dirty: bool,
    /// Per-window pitch/rate slider (`Rate: 0,00` in Sound Forge bottom bar, -12..+12)
    pub rate_slider: f32,
    /// Channel active mask `[Left, Right]` toggled via the blue `[1] [-]` and `[2] [-]` badges.
    pub channel_enabled: [bool; 2],
    /// Whether this MDI window is collapsed/minimized when stacked.
    pub minimized: bool,
    /// Individual MDI window height in pixels (can be dragged thin ~115px or thick ~680px!).
    pub win_height: f32,
    /// Individual MDI window width ratio (0.35 .. 1.00 of workspace width).
    pub win_width_ratio: f32,
    /// Vertical waveform amplitude zoom (0.25x thinner .. 4.0x thicker).
    pub amp_zoom: f32,
}

impl AudioDocument {
    pub fn from_buffer(
        id: usize,
        title: String,
        path: Option<PathBuf>,
        buffer: AudioBuffer,
    ) -> Self {
        let total = buffer.num_frames().max(1) as f64;
        let pyramid = WaveformPyramid::build(&buffer);
        Self {
            id,
            title,
            path,
            buffer: Arc::new(buffer),
            pyramid,
            annotations: AnnotationSet::default(),
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            cursor_frame: 0,
            selection: None,
            view_start: 0.0,
            view_frames: total,
            dirty: false,
            rate_slider: 0.0,
            channel_enabled: [true, true],
            minimized: false,
            win_height: 235.0,
            win_width_ratio: 0.98,
            amp_zoom: 1.0,
        }
    }

    pub fn formatted_title_with_duration(&self) -> String {
        let secs = self.buffer.duration_secs().round() as u64;
        let m = secs / 60;
        let s = secs % 60;
        let dirty_mark = if self.dirty { " *" } else { "" };
        if m > 0 {
            format!("{} ({}m {:02}s){}", self.title, m, s, dirty_mark)
        } else {
            format!("{} ({}s){}", self.title, s, dirty_mark)
        }
    }

    /// Converts per-window `Rate` (-12.0 .. +12.0 semitones) to playback speed factor.
    pub fn rate_to_speed_factor(&self) -> f64 {
        2.0_f64.powf(self.rate_slider as f64 / 12.0)
    }

    pub fn push_undo(&mut self) {
        if self.undo_stack.len() >= 25 {
            self.undo_stack.remove(0);
        }
        self.undo_stack.push(UndoSnapshot {
            buffer: self.buffer.clone(),
            annotations: self.annotations.clone(),
            cursor_frame: self.cursor_frame,
            selection: self.selection,
        });
        self.redo_stack.clear();
    }

    pub fn commit_buffer(
        &mut self,
        new_buffer: AudioBuffer,
        new_cursor: usize,
        new_selection: Option<(usize, usize)>,
    ) {
        self.push_undo();
        self.buffer = Arc::new(new_buffer);
        self.pyramid = WaveformPyramid::build(&self.buffer);
        let n = self.buffer.num_frames();
        self.cursor_frame = new_cursor.min(n);
        self.selection = new_selection.and_then(|(a, b)| {
            let s = a.min(n);
            let e = b.min(n);
            if e > s {
                Some((s, e))
            } else {
                None
            }
        });
        self.clamp_viewport();
        self.dirty = true;
    }

    pub fn undo(&mut self) -> bool {
        if let Some(snap) = self.undo_stack.pop() {
            self.redo_stack.push(UndoSnapshot {
                buffer: self.buffer.clone(),
                annotations: self.annotations.clone(),
                cursor_frame: self.cursor_frame,
                selection: self.selection,
            });
            self.buffer = snap.buffer;
            self.pyramid = WaveformPyramid::build(&self.buffer);
            self.annotations = snap.annotations;
            self.cursor_frame = snap.cursor_frame;
            self.selection = snap.selection;
            self.clamp_viewport();
            self.dirty = true;
            true
        } else {
            false
        }
    }

    pub fn redo(&mut self) -> bool {
        if let Some(snap) = self.redo_stack.pop() {
            self.undo_stack.push(UndoSnapshot {
                buffer: self.buffer.clone(),
                annotations: self.annotations.clone(),
                cursor_frame: self.cursor_frame,
                selection: self.selection,
            });
            self.buffer = snap.buffer;
            self.pyramid = WaveformPyramid::build(&self.buffer);
            self.annotations = snap.annotations;
            self.cursor_frame = snap.cursor_frame;
            self.selection = snap.selection;
            self.clamp_viewport();
            self.dirty = true;
            true
        } else {
            false
        }
    }

    pub fn target_range(&self) -> (usize, usize) {
        if let Some((s, e)) = self.selection {
            if e > s {
                return (s, e);
            }
        }
        (0, self.buffer.num_frames())
    }

    pub fn clamp_viewport(&mut self) {
        let total = self.buffer.num_frames().max(1) as f64;
        self.view_frames = self.view_frames.clamp(32.0_f64.min(total), total);
        self.view_start = self
            .view_start
            .clamp(0.0, (total - self.view_frames).max(0.0));
    }

    pub fn zoom_at(&mut self, factor: f64, anchor_ratio: f64) {
        let total = self.buffer.num_frames().max(1) as f64;
        let anchor_ratio = anchor_ratio.clamp(0.0, 1.0);
        let anchor_frame = self.view_start + self.view_frames * anchor_ratio;
        let min_frames = 32.0_f64.min(total);
        let new_frames = (self.view_frames * factor).clamp(min_frames, total);
        self.view_frames = new_frames;
        self.view_start = anchor_frame - new_frames * anchor_ratio;
        self.clamp_viewport();
    }

    pub fn zoom_fit(&mut self) {
        self.view_start = 0.0;
        self.view_frames = self.buffer.num_frames().max(1) as f64;
    }

    pub fn zoom_selection(&mut self) {
        if let Some((s, e)) = self.selection {
            if e > s {
                let len = (e - s) as f64;
                let pad = (len * 0.05).max(16.0);
                self.view_start = (s as f64 - pad).max(0.0);
                self.view_frames = len + pad * 2.0;
                self.clamp_viewport();
            }
        }
    }
}
