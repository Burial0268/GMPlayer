use serde::{Deserialize, Serialize};

pub const DEFAULT_ADDTIONAL_WINDOW_ARGS: &str = "--enable-gpu-rasterization --enable-zero-copy --ignore-gpu-blocklist --use-gl=angle --disable-features=VaapiVideoDecoder,UseChromeOSDirectVideoDecoder,msWebOOUI,msPdfOOUI --enable-threaded-compositing --num-raster-threads=4";

/// Known window labels with preset configurations.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum WindowLabel {
    Main,
    MiniPlayer,
    DesktopLyrics,
    Settings,
    About,
    TrayPopup,
    Custom(String),
}

impl WindowLabel {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Main => "main",
            Self::MiniPlayer => "mini-player",
            Self::DesktopLyrics => "desktop-lyrics",
            Self::Settings => "settings",
            Self::About => "about",
            Self::TrayPopup => "tray-popup",
            Self::Custom(s) => s.as_str(),
        }
    }

    pub fn parse(s: &str) -> Self {
        match s {
            "main" => Self::Main,
            "mini-player" => Self::MiniPlayer,
            "desktop-lyrics" => Self::DesktopLyrics,
            "settings" => Self::Settings,
            "about" => Self::About,
            "tray-popup" => Self::TrayPopup,
            other => Self::Custom(other.to_string()),
        }
    }
}

/// Configuration for creating a window.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowConfig {
    pub label: String,
    pub title: String,
    pub url: String,
    pub width: f64,
    pub height: f64,
    #[serde(default)]
    pub min_width: Option<f64>,
    #[serde(default)]
    pub min_height: Option<f64>,
    #[serde(default)]
    pub max_width: Option<f64>,
    #[serde(default)]
    pub max_height: Option<f64>,
    #[serde(default = "default_true")]
    pub resizable: bool,
    #[serde(default)]
    pub decorations: bool,
    #[serde(default)]
    pub transparent: bool,
    #[serde(default)]
    pub always_on_top: bool,
    #[serde(default)]
    pub skip_taskbar: bool,
    #[serde(default)]
    pub center: bool,
    #[serde(default = "default_true")]
    pub visible: bool,
    /// If true, reuse existing window instead of creating a duplicate.
    #[serde(default)]
    pub single_instance: bool,
    /// If true, close button hides the window instead of destroying it.
    #[serde(default)]
    pub closeable_to_tray: bool,
    /// If true, apply decorum overlay titlebar.
    #[serde(default)]
    pub use_overlay_titlebar: bool,
    /// macOS traffic lights inset (x, y). Only used on macOS.
    #[serde(default)]
    pub traffic_lights_inset: Option<(f64, f64)>,
    /// Native window effect to apply (e.g. "acrylic"). Platform-specific.
    #[serde(default)]
    pub window_effect: Option<String>,
    /// Whether to show a native window shadow. Defaults to false for transparent windows.
    #[serde(default)]
    pub shadow: bool,
    /// Additional args for window
    #[serde(default)]
    pub additional_args: Option<String>
}

fn default_true() -> bool {
    true
}

impl WindowConfig {
    /// Main window preset — the primary app window.
    pub fn main() -> Self {
        Self {
            label: "main".into(),
            title: "GMPlayer".into(),
            url: "/".into(),
            width: 881.0,
            height: 653.0,
            min_width: Some(680.0),
            min_height: Some(500.0),
            max_width: None,
            max_height: None,
            resizable: true,
            decorations: false,
            transparent: false,
            always_on_top: false,
            skip_taskbar: false,
            center: false,
            visible: true,
            single_instance: true,
            closeable_to_tray: true,
            use_overlay_titlebar: true,
            traffic_lights_inset: Some((12.0, 16.0)),
            window_effect: None,
            shadow: true,
            additional_args: Some(DEFAULT_ADDTIONAL_WINDOW_ARGS.to_owned())
        }
    }

    /// Mini player preset — compact always-on-top player.
    pub fn mini_player() -> Self {
        Self {
            label: "mini-player".into(),
            title: "Mini Player".into(),
            url: "/slave.html#/mini-player".into(),
            width: 350.0,
            height: 80.0,
            min_width: None,
            min_height: None,
            max_width: None,
            max_height: None,
            resizable: false,
            decorations: false,
            transparent: true,
            always_on_top: true,
            skip_taskbar: true,
            center: false,
            visible: true,
            single_instance: true,
            closeable_to_tray: false,
            use_overlay_titlebar: false,
            traffic_lights_inset: None,
            window_effect: Some("acrylic".into()),
            shadow: true,
            additional_args: Some(DEFAULT_ADDTIONAL_WINDOW_ARGS.to_owned())
        }
    }

    /// Desktop lyrics preset — floating lyrics overlay.
    pub fn desktop_lyrics() -> Self {
        Self {
            label: "desktop-lyrics".into(),
            title: "Desktop Lyrics".into(),
            url: "/slave.html#/desktop-lyrics".into(),
            width: 800.0,
            height: 120.0,
            min_width: Some(400.0),
            min_height: Some(60.0),
            max_width: None,
            max_height: None,
            resizable: true,
            decorations: false,
            transparent: true,
            always_on_top: true,
            skip_taskbar: true,
            center: false,
            visible: true,
            single_instance: true,
            closeable_to_tray: false,
            use_overlay_titlebar: false,
            traffic_lights_inset: None,
            window_effect: None,
            shadow: false,
            additional_args: Some(DEFAULT_ADDTIONAL_WINDOW_ARGS.to_owned())
        }
    }

    /// Settings window preset.
    pub fn settings() -> Self {
        Self {
            label: "settings".into(),
            title: "Settings".into(),
            url: "/setting".into(),
            width: 700.0,
            height: 500.0,
            min_width: Some(500.0),
            min_height: Some(400.0),
            max_width: None,
            max_height: None,
            resizable: true,
            decorations: false,
            transparent: false,
            always_on_top: false,
            skip_taskbar: false,
            center: true,
            visible: true,
            single_instance: true,
            closeable_to_tray: false,
            use_overlay_titlebar: true,
            traffic_lights_inset: Some((12.0, 16.0)),
            window_effect: None,
            shadow: true,
            additional_args: None
        }
    }

    /// About window preset.
    pub fn about() -> Self {
        Self {
            label: "about".into(),
            title: "About".into(),
            url: "/about".into(),
            width: 400.0,
            height: 350.0,
            min_width: None,
            min_height: None,
            max_width: None,
            max_height: None,
            resizable: false,
            decorations: false,
            transparent: false,
            always_on_top: true,
            skip_taskbar: false,
            center: true,
            visible: true,
            single_instance: true,
            closeable_to_tray: false,
            use_overlay_titlebar: true,
            traffic_lights_inset: Some((12.0, 16.0)),
            window_effect: None,
            shadow: true,
            additional_args: None
        }
    }

    /// Tray popup preset — small borderless popup shown near the system tray.
    /// Uses a standalone HTML file (not the SPA) to avoid loading Vue/Pinia/AudioContext.
    pub fn tray_popup() -> Self {
        Self {
            label: "tray-popup".into(),
            title: "Tray Popup".into(),
            url: "/tray-popup.html".into(),
            width: 260.0,
            height: 310.0,
            min_width: None,
            min_height: None,
            max_width: None,
            max_height: None,
            resizable: false,
            decorations: false,
            transparent: true,
            always_on_top: true,
            skip_taskbar: true,
            center: false,
            visible: false,
            single_instance: true,
            closeable_to_tray: false,
            use_overlay_titlebar: false,
            traffic_lights_inset: None,
            window_effect: Some("acrylic".into()),
            shadow: true,
            additional_args: Some(DEFAULT_ADDTIONAL_WINDOW_ARGS.to_owned())
        }
    }

    /// Look up a preset by label string. Returns None for unknown labels.
    pub fn from_label(label: &str) -> Option<Self> {
        match label {
            "main" => Some(Self::main()),
            "mini-player" => Some(Self::mini_player()),
            "desktop-lyrics" => Some(Self::desktop_lyrics()),
            "settings" => Some(Self::settings()),
            "about" => Some(Self::about()),
            "tray-popup" => Some(Self::tray_popup()),
            _ => None,
        }
    }
}
