use egui::{Color32, Rounding, Stroke, Style, Visuals};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UiSkin {
    /// Authentic Sony Sound Forge Pro 10.0 light/silver chrome + white canvas + navy waveform.
    ClassicSoundForge10,
    /// Dark studio theme.
    DarkStudio,
}

pub struct Palette {
    pub panel_bg: Color32,
    pub toolbar_bg: Color32,
    pub mdi_bg: Color32,
    pub win_title_active: Color32,
    pub win_title_inactive: Color32,
    pub win_title_text: Color32,
    pub canvas_bg: Color32,
    pub canvas_alt_bg: Color32,
    pub ruler_bg: Color32,
    pub border: Color32,
    pub grid_major: Color32,
    pub grid_zero: Color32,
    pub wave_fill: Color32,
    pub wave_outline: Color32,
    pub selection_fill: Color32,
    pub selection_border: Color32,
    pub playhead: Color32,
    pub cursor: Color32,
    pub marker: Color32,
    pub region_fill: Color32,
    pub region_border: Color32,
    pub channel_badge_bg: Color32,
    pub text_main: Color32,
    pub text_muted: Color32,
}

impl UiSkin {
    pub fn palette(&self) -> Palette {
        match self {
            UiSkin::ClassicSoundForge10 => Palette {
                panel_bg: Color32::from_rgb(240, 240, 240),
                toolbar_bg: Color32::from_rgb(232, 234, 238),
                mdi_bg: Color32::from_rgb(171, 171, 171),
                win_title_active: Color32::from_rgb(185, 209, 234),
                win_title_inactive: Color32::from_rgb(215, 220, 228),
                win_title_text: Color32::from_rgb(20, 24, 32),
                canvas_bg: Color32::from_rgb(255, 255, 255),
                canvas_alt_bg: Color32::from_rgb(252, 253, 255),
                ruler_bg: Color32::from_rgb(236, 238, 242),
                border: Color32::from_rgb(160, 166, 176),
                grid_major: Color32::from_rgba_premultiplied(200, 206, 216, 180),
                grid_zero: Color32::from_rgb(110, 115, 125),
                // Classic Sound Forge 10 Deep Navy Blue waveform!
                wave_fill: Color32::from_rgb(0, 20, 120),
                wave_outline: Color32::from_rgb(0, 28, 158),
                selection_fill: Color32::from_rgba_premultiplied(50, 115, 220, 62),
                selection_border: Color32::from_rgb(20, 85, 190),
                playhead: Color32::from_rgb(210, 30, 30),
                cursor: Color32::from_rgb(20, 20, 20),
                // Classic Sound Forge 10 Orange Marker Flags (#E87828)
                marker: Color32::from_rgb(232, 120, 40),
                region_fill: Color32::from_rgba_premultiplied(232, 120, 40, 32),
                region_border: Color32::from_rgb(210, 95, 20),
                channel_badge_bg: Color32::from_rgb(45, 85, 185),
                text_main: Color32::from_rgb(20, 22, 26),
                text_muted: Color32::from_rgb(85, 92, 102),
            },
            UiSkin::DarkStudio => Palette {
                panel_bg: Color32::from_rgb(30, 33, 39),
                toolbar_bg: Color32::from_rgb(38, 42, 50),
                mdi_bg: Color32::from_rgb(22, 24, 29),
                win_title_active: Color32::from_rgb(46, 82, 130),
                win_title_inactive: Color32::from_rgb(42, 46, 56),
                win_title_text: Color32::from_rgb(235, 240, 248),
                canvas_bg: Color32::from_rgb(18, 21, 26),
                canvas_alt_bg: Color32::from_rgb(22, 26, 32),
                ruler_bg: Color32::from_rgb(28, 32, 38),
                border: Color32::from_rgb(56, 62, 74),
                grid_major: Color32::from_rgba_premultiplied(80, 92, 110, 80),
                grid_zero: Color32::from_rgba_premultiplied(110, 130, 155, 130),
                wave_fill: Color32::from_rgb(36, 184, 116),
                wave_outline: Color32::from_rgb(94, 242, 174),
                selection_fill: Color32::from_rgba_premultiplied(45, 130, 215, 70),
                selection_border: Color32::from_rgb(80, 170, 255),
                playhead: Color32::from_rgb(255, 92, 92),
                cursor: Color32::from_rgb(235, 240, 248),
                marker: Color32::from_rgb(255, 176, 32),
                region_fill: Color32::from_rgba_premultiplied(255, 176, 32, 32),
                region_border: Color32::from_rgb(255, 186, 66),
                channel_badge_bg: Color32::from_rgb(45, 95, 195),
                text_main: Color32::from_rgb(230, 235, 242),
                text_muted: Color32::from_rgb(150, 160, 175),
            },
        }
    }
}

pub struct ForgeTheme;

impl ForgeTheme {
    pub const ACCENT_GREEN: Color32 = Color32::from_rgb(34, 165, 95);
    pub const ACCENT_AMBER: Color32 = Color32::from_rgb(225, 135, 20);
    pub const ACCENT_RED: Color32 = Color32::from_rgb(215, 45, 45);
}

pub fn apply_forge_style(ctx: &egui::Context, skin: UiSkin) {
    let p = skin.palette();
    let mut style = Style::default();
    let mut visuals = match skin {
        UiSkin::ClassicSoundForge10 => Visuals::light(),
        UiSkin::DarkStudio => Visuals::dark(),
    };

    visuals.window_fill = p.panel_bg;
    visuals.panel_fill = p.panel_bg;
    visuals.faint_bg_color = p.toolbar_bg;
    visuals.extreme_bg_color = p.canvas_bg;

    visuals.widgets.noninteractive.bg_fill = p.toolbar_bg;
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, p.border);
    visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0_f32, p.text_main);

    match skin {
        UiSkin::ClassicSoundForge10 => {
            visuals.widgets.inactive.bg_fill = Color32::from_rgb(225, 228, 234);
            visuals.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, p.border);
            visuals.widgets.inactive.fg_stroke = Stroke::new(1.0_f32, p.text_main);
            visuals.widgets.hovered.bg_fill = Color32::from_rgb(205, 222, 245);
            visuals.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, p.selection_border);
            visuals.widgets.active.bg_fill = Color32::from_rgb(175, 205, 240);
            visuals.selection.bg_fill = Color32::from_rgb(51, 153, 255);
            visuals.selection.stroke = Stroke::new(1.0_f32, Color32::WHITE);
        }
        UiSkin::DarkStudio => {
            visuals.widgets.inactive.bg_fill = Color32::from_rgb(48, 53, 64);
            visuals.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, p.border);
            visuals.widgets.hovered.bg_fill = Color32::from_rgb(62, 70, 85);
            visuals.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, p.selection_border);
            visuals.widgets.active.bg_fill = Color32::from_rgb(42, 110, 175);
            visuals.selection.bg_fill = Color32::from_rgb(42, 115, 185);
        }
    }

    visuals.widgets.inactive.rounding = Rounding::same(2.0);
    visuals.widgets.hovered.rounding = Rounding::same(2.0);
    visuals.widgets.active.rounding = Rounding::same(2.0);

    style.visuals = visuals;
    style.spacing.item_spacing = egui::vec2(5.0, 4.0);
    style.spacing.button_padding = egui::vec2(6.0, 3.0);
    ctx.set_style(style);
}

pub fn format_timecode(secs: f64) -> String {
    let secs = secs.max(0.0);
    let total_ms = (secs * 1000.0).round() as u64;
    let ms = total_ms % 1000;
    let total_s = total_ms / 1000;
    let s = total_s % 60;
    let m = (total_s / 60) % 60;
    let h = total_s / 3600;
    format!("{h:02}:{m:02}:{s:02},{ms:03}")
}

pub fn format_ruler_time(secs: f64) -> String {
    let secs = secs.max(0.0);
    let total_ms = (secs * 1000.0).round() as u64;
    let ms = total_ms % 1000;
    let total_s = total_ms / 1000;
    let s = total_s % 60;
    let m = (total_s / 60) % 60;
    let h = total_s / 3600;
    if ms == 0 {
        format!("{h:02}:{m:02}:{s:02}")
    } else {
        format!("{h:02}:{m:02}:{s:02},{ms:03}")
    }
}

pub fn format_timecode_dot(secs: f64) -> String {
    let secs = secs.max(0.0);
    let total_ms = (secs * 1000.0).round() as u64;
    let ms = total_ms % 1000;
    let total_s = total_ms / 1000;
    let s = total_s % 60;
    let m = (total_s / 60) % 60;
    let h = total_s / 3600;
    format!("{h:02}:{m:02}:{s:02}.{ms:03}")
}

/// Parses `HH:MM:SS.mmm`, `MM:SS.mmm`, `HH:MM:SS,mmm`, or raw seconds `95.981` into seconds.
pub fn parse_timecode_secs(input: &str) -> Option<f64> {
    let clean = input.trim().replace(',', ".");
    if clean.is_empty() {
        return None;
    }
    let parts: Vec<&str> = clean.split(':').collect();
    match parts.len() {
        1 => parts[0].parse::<f64>().ok().filter(|&v| v > 0.0),
        2 => {
            let m = parts[0].parse::<f64>().ok()?;
            let s = parts[1].parse::<f64>().ok()?;
            Some(m * 60.0 + s).filter(|&v| v > 0.0)
        }
        3 => {
            let h = parts[0].parse::<f64>().ok()?;
            let m = parts[1].parse::<f64>().ok()?;
            let s = parts[2].parse::<f64>().ok()?;
            Some(h * 3600.0 + m * 60.0 + s).filter(|&v| v > 0.0)
        }
        _ => None,
    }
}

pub fn amp_to_db(amp: f32) -> f32 {
    if amp <= 1e-6 {
        -96.0
    } else {
        20.0 * amp.log10()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_timecode_secs() {
        let s1 = parse_timecode_secs("00:01:35.981").unwrap();
        assert!((s1 - 95.981).abs() < 1e-4);

        let s2 = parse_timecode_secs("01:46,824").unwrap();
        assert!((s2 - 106.824).abs() < 1e-4);
    }
}
