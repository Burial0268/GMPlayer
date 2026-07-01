use serde::Serialize;

/// Returns whether the app is running on desktop (non-mobile) targets.
#[tauri::command(rename_all = "snake_case")]
pub fn detect_desktop() -> bool {
    !cfg!(mobile)
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopEnvironment {
    pub os: &'static str,
    pub family: &'static str,
    pub desktop: Option<String>,
    pub session_type: Option<String>,
    pub is_mobile: bool,
    pub is_macos: bool,
    pub is_linux: bool,
    pub is_hyprland: bool,
    pub uses_native_traffic_lights: bool,
}

/// Returns platform details needed by the frontend window chrome.
#[tauri::command(rename_all = "snake_case")]
pub fn desktop_environment() -> DesktopEnvironment {
    let desktop = env_first(&[
        "XDG_CURRENT_DESKTOP",
        "XDG_SESSION_DESKTOP",
        "DESKTOP_SESSION",
    ]);
    let session_type = env_first(&["XDG_SESSION_TYPE"]);
    let is_hyprland = cfg!(target_os = "linux")
        && (std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE").is_some()
            || contains_hyprland(desktop.as_deref())
            || contains_hyprland(session_type.as_deref()));

    DesktopEnvironment {
        os: std::env::consts::OS,
        family: std::env::consts::FAMILY,
        desktop,
        session_type,
        is_mobile: cfg!(mobile),
        is_macos: cfg!(target_os = "macos"),
        is_linux: cfg!(target_os = "linux"),
        is_hyprland,
        uses_native_traffic_lights: cfg!(target_os = "macos") && !cfg!(mobile),
    }
}

fn env_first(keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|key| {
        let value = std::env::var(key).ok()?.trim().to_string();
        (!value.is_empty()).then_some(value)
    })
}

fn contains_hyprland(value: Option<&str>) -> bool {
    value
        .map(|value| value.to_ascii_lowercase().contains("hyprland"))
        .unwrap_or(false)
}
