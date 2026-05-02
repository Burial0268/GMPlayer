/// Returns whether the app is running on desktop (non-mobile) targets.
#[tauri::command(rename_all = "snake_case")]
pub fn detect_desktop() -> bool {
    !cfg!(mobile)
}
