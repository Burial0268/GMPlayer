use serde::{Deserialize, Serialize};

#[cfg(target_os = "windows")]
pub const DEFAULT_ADDITIONAL_WINDOW_ARGS: &str = "--enable-gpu-rasterization --enable-zero-copy --ignore-gpu-blocklist --use-gl=angle --disable-features=VaapiVideoDecoder,UseChromeOSDirectVideoDecoder,msWebOOUI,msPdfOOUI --enable-threaded-compositing --num-raster-threads=4";

/// Known window labels with preset configurations.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum WindowLabel {
    Main,
    MiniPlayer,
    DesktopLyrics,
    DesktopLyricsControls,
    #[cfg(windows)]
    TaskbarLyric,
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
            Self::DesktopLyricsControls => "desktop-lyrics-controls",
            #[cfg(windows)]
            Self::TaskbarLyric => "taskbar-lyric",
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
            "desktop-lyrics-controls" => Self::DesktopLyricsControls,
            #[cfg(windows)]
            "taskbar-lyric" => Self::TaskbarLyric,
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
    pub additional_args: Option<String>,
    /// Parent window label (for child windows)
    #[serde(default)]
    pub parent_label: Option<String>,
}

fn default_true() -> bool {
    true
}

#[cfg(target_os = "windows")]
fn default_additional_window_args() -> Option<String> {
    Some(DEFAULT_ADDITIONAL_WINDOW_ARGS.to_owned())
}

#[cfg(not(target_os = "windows"))]
fn default_additional_window_args() -> Option<String> {
    None
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
            min_width: Some(800.0),
            min_height: Some(600.0),
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
            window_effect: Some("acrylic".into()),
            shadow: true,
            additional_args: default_additional_window_args(),
            parent_label: None,
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
            additional_args: default_additional_window_args(),
            parent_label: None,
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
            additional_args: default_additional_window_args(),
            parent_label: None,
        }
    }

    /// Desktop lyrics controls preset — child window for controls.
    pub fn desktop_lyrics_controls() -> Self {
        Self {
            label: "desktop-lyrics-controls".into(),
            title: "Desktop Lyrics Controls".into(),
            url: "/slave.html#/desktop-lyrics-controls".into(),
            width: 220.0,
            height: 40.0,
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
            window_effect: None,
            shadow: false,
            additional_args: default_additional_window_args(),
            parent_label: Some("desktop-lyrics".into()),
        }
    }

    /// Taskbar lyric preset — Windows-only webview embedded into the taskbar.
    ///
    /// This preset registers the label and basic webview shape with the normal
    /// window registry. The actual creation path still lives in the taskbar
    /// lyric plugin because it must embed the native HWND and manage taskbar
    /// watchers/mouse forwarding around the window lifecycle.
    #[cfg(windows)]
    pub fn taskbar_lyric() -> Self {
        Self {
            label: "taskbar-lyric".into(),
            title: "Taskbar Lyric".into(),
            url: "/slave.html#/taskbar-lyric".into(),
            width: 320.0,
            height: 48.0,
            min_width: None,
            min_height: None,
            max_width: None,
            max_height: None,
            resizable: false,
            decorations: true,
            transparent: true,
            always_on_top: true,
            skip_taskbar: false,
            center: false,
            visible: true,
            single_instance: true,
            closeable_to_tray: false,
            use_overlay_titlebar: false,
            traffic_lights_inset: None,
            window_effect: None,
            shadow: false,
            additional_args: default_additional_window_args(),
            parent_label: None,
        }
    }

    /// Settings window preset.
    pub fn settings() -> Self {
        Self {
            label: "settings".into(),
            title: "Settings".into(),
            url: "/slave.html#/settings".into(),
            width: 860.0,
            height: 620.0,
            min_width: Some(680.0),
            min_height: Some(520.0),
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
            additional_args: default_additional_window_args(),
            parent_label: None,
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
            additional_args: default_additional_window_args(),
            parent_label: None,
        }
    }

    /// Tray popup preset — small borderless popup shown near the system tray.
    /// Uses the lightweight slave Vue entry to avoid loading the main app stores/player.
    pub fn tray_popup() -> Self {
        Self {
            label: "tray-popup".into(),
            title: "Tray Popup".into(),
            url: "/slave.html#/tray-popup".into(),
            width: 260.0,
            height: 370.0,
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
            additional_args: default_additional_window_args(),
            parent_label: None,
        }
    }

    /// Look up a preset by label string. Returns None for unknown labels.
    pub fn from_label(label: &str) -> Option<Self> {
        match label {
            "main" => Some(Self::main()),
            "mini-player" => Some(Self::mini_player()),
            "desktop-lyrics" => Some(Self::desktop_lyrics()),
            "desktop-lyrics-controls" => Some(Self::desktop_lyrics_controls()),
            #[cfg(windows)]
            "taskbar-lyric" => Some(Self::taskbar_lyric()),
            "settings" => Some(Self::settings()),
            "about" => Some(Self::about()),
            "tray-popup" => Some(Self::tray_popup()),
            _ => None,
        }
    }

    /// Effective browser args for WebView2-backed windows.
    ///
    /// WebView2 requires every webview that shares the same user data folder
    /// to be created with the same additional browser arguments. A custom or
    /// incomplete window config with `additional_args = None` can otherwise
    /// fail to open after the main profile has already been initialized.
    #[cfg(target_os = "windows")]
    pub fn effective_additional_args(&self) -> Option<&str> {
        Some(DEFAULT_ADDITIONAL_WINDOW_ARGS)
    }

    #[cfg(not(target_os = "windows"))]
    pub fn effective_additional_args(&self) -> Option<&str> {
        self.additional_args.as_deref()
    }
}
