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
}

impl AudioDocument {
    pub fn from_buffer(id: usize, title: String, path: Option<PathBuf>, buffer: AudioBuffer) -> Self {
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
        }
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

    /// Returns active selection [start, end), or full buffer [0, num_frames) if nothing is selected.
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
        self.view_frames = self.view_frames.clamp(64.0_f64.min(total), total);
        self.view_start = self.view_start.clamp(0.0, (total - self.view_frames).max(0.0));
    }

    pub fn zoom_at(&mut self, factor: f64, anchor_ratio: f64) {
        let total = self.buffer.num_frames().max(1) as f64;
        let anchor_ratio = anchor_ratio.clamp(0.0, 1.0);
        let anchor_frame = self.view_start + self.view_frames * anchor_ratio;
        let min_frames = 64.0_f64.min(total);
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
