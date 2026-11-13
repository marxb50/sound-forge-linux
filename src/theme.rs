use egui::{Color32, Rounding, Stroke, Style, Visuals};

pub struct ForgeTheme;

impl ForgeTheme {
    pub const PANEL_BG: Color32 = Color32::from_rgb(30, 33, 39);
    pub const TOOLBAR_BG: Color32 = Color32::from_rgb(38, 42, 50);
    pub const CANVAS_BG: Color32 = Color32::from_rgb(18, 21, 26);
    pub const CANVAS_ALT_BG: Color32 = Color32::from_rgb(22, 26, 32);
    pub const RULER_BG: Color32 = Color32::from_rgb(28, 32, 38);
    pub const BORDER: Color32 = Color32::from_rgb(56, 62, 74);
    pub const GRID_MAJOR: Color32 = Color32::from_rgba_premultiplied(80, 92, 110, 80);
    pub const GRID_ZERO: Color32 = Color32::from_rgba_premultiplied(110, 130, 155, 130);

    // Classic Sound Forge inspired emerald/cyan waveform
    pub const WAVE_FILL: Color32 = Color32::from_rgb(36, 184, 116);
    pub const WAVE_OUTLINE: Color32 = Color32::from_rgb(94, 242, 174);
    pub const WAVE_RMS_HINT: Color32 = Color32::from_rgb(58, 214, 144);

    // Selection & Playhead & Markers
    pub const SELECTION_FILL: Color32 = Color32::from_rgba_premultiplied(45, 130, 215, 70);
    pub const SELECTION_BORDER: Color32 = Color32::from_rgb(80, 170, 255);
    pub const PLAYHEAD: Color32 = Color32::from_rgb(255, 92, 92);
    pub const CURSOR: Color32 = Color32::from_rgb(235, 240, 248);
    pub const MARKER: Color32 = Color32::from_rgb(255, 176, 32);
    pub const REGION_FILL: Color32 = Color32::from_rgba_premultiplied(255, 176, 32, 32);
    pub const REGION_BORDER: Color32 = Color32::from_rgb(255, 186, 66);

    pub const ACCENT_GREEN: Color32 = Color32::from_rgb(52, 211, 153);
    pub const ACCENT_AMBER: Color32 = Color32::from_rgb(251, 191, 36);
    pub const ACCENT_RED: Color32 = Color32::from_rgb(239, 68, 68);
    pub const TEXT_MUTED: Color32 = Color32::from_rgb(150, 160, 175);
}

pub fn apply_forge_style(ctx: &egui::Context) {
    let mut style = Style::default();
    let mut visuals = Visuals::dark();

    visuals.window_fill = ForgeTheme::PANEL_BG;
    visuals.panel_fill = ForgeTheme::PANEL_BG;
    visuals.faint_bg_color = ForgeTheme::TOOLBAR_BG;
    visuals.extreme_bg_color = ForgeTheme::CANVAS_BG;

    visuals.widgets.noninteractive.bg_fill = ForgeTheme::TOOLBAR_BG;
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, ForgeTheme::BORDER);
    visuals.widgets.inactive.bg_fill = Color32::from_rgb(48, 53, 64);
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, ForgeTheme::BORDER);
    visuals.widgets.hovered.bg_fill = Color32::from_rgb(62, 70, 85);
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, ForgeTheme::SELECTION_BORDER);
    visuals.widgets.active.bg_fill = Color32::from_rgb(42, 110, 175);

    visuals.selection.bg_fill = Color32::from_rgb(42, 115, 185);
    visuals.widgets.inactive.rounding = Rounding::same(3.0);
    visuals.widgets.hovered.rounding = Rounding::same(3.0);
    visuals.widgets.active.rounding = Rounding::same(3.0);

    style.visuals = visuals;
    style.spacing.item_spacing = egui::vec2(6.0, 5.0);
    style.spacing.button_padding = egui::vec2(8.0, 4.0);
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
    if h > 0 {
        format!("{h:02}:{m:02}:{s:02}.{ms:03}")
    } else {
        format!("{m:02}:{s:02}.{ms:03}")
    }
}

pub fn amp_to_db(amp: f32) -> f32 {
    if amp <= 1e-6 {
        -96.0
    } else {
        20.0 * amp.log10()
    }
}
