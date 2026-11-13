use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Marker {
    pub id: usize,
    pub name: String,
    pub frame: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Region {
    pub id: usize,
    pub name: String,
    pub start_frame: usize,
    pub end_frame: usize,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct AnnotationSet {
    pub markers: Vec<Marker>,
    pub regions: Vec<Region>,
    pub next_id: usize,
}

impl AnnotationSet {
    pub fn add_marker(&mut self, frame: usize, custom_name: Option<String>) -> usize {
        self.next_id += 1;
        let id = self.next_id;
        let name = custom_name.unwrap_or_else(|| format!("Marcador {id}"));
        self.markers.push(Marker { id, name, frame });
        self.markers.sort_by_key(|m| m.frame);
        id
    }

    pub fn add_region(
        &mut self,
        start_frame: usize,
        end_frame: usize,
        custom_name: Option<String>,
    ) -> Option<usize> {
        let (s, e) = if start_frame <= end_frame {
            (start_frame, end_frame)
        } else {
            (end_frame, start_frame)
        };
        if s == e {
            return None;
        }
        self.next_id += 1;
        let id = self.next_id;
        let name = custom_name.unwrap_or_else(|| format!("Região {id}"));
        self.regions.push(Region {
            id,
            name,
            start_frame: s,
            end_frame: e,
        });
        self.regions.sort_by_key(|r| r.start_frame);
        Some(id)
    }

    pub fn remove_marker(&mut self, id: usize) {
        self.markers.retain(|m| m.id != id);
    }

    pub fn remove_region(&mut self, id: usize) {
        self.regions.retain(|r| r.id != id);
    }

    /// Shifts or clamps markers/regions after an audio edit at [edit_start, edit_end) replaced with `new_len` frames.
    pub fn adjust_for_edit(&mut self, edit_start: usize, edit_end: usize, new_len: usize) {
        let old_len = edit_end.saturating_sub(edit_start);
        let delta = new_len as isize - old_len as isize;

        for m in &mut self.markers {
            if m.frame >= edit_end {
                m.frame = (m.frame as isize + delta).max(edit_start as isize) as usize;
            } else if m.frame > edit_start {
                m.frame = edit_start + new_len.min(m.frame - edit_start);
            }
        }

        for r in &mut self.regions {
            if r.start_frame >= edit_end {
                r.start_frame = (r.start_frame as isize + delta).max(0) as usize;
                r.end_frame = (r.end_frame as isize + delta).max(0) as usize;
            } else if r.end_frame > edit_start {
                if r.end_frame >= edit_end {
                    r.end_frame = (r.end_frame as isize + delta).max(r.start_frame as isize) as usize;
                } else {
                    r.end_frame = r.end_frame.min(edit_start + new_len);
                }
            }
        }
        self.regions.retain(|r| r.end_frame > r.start_frame);
    }
}
