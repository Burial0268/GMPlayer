//! Sharing a lyric card out of the app: the native save dialog on desktop, the
//! system share sheet on Android. The PNG arrives as a raw IPC body (a JSON
//! array of a megabyte of bytes would be four times the size); the suggested
//! file name rides in an `x-share` header as a URL query, because a header
//! value must be Latin-1 and song titles are not.

use tauri::ipc::{InvokeBody, Request};

fn png_and_name<'a>(request: &'a Request<'_>) -> Result<(&'a [u8], String), String> {
    let InvokeBody::Raw(png) = request.body() else {
        return Err("expected a raw PNG body".into());
    };
    let name = request
        .headers()
        .get("x-share")
        .and_then(|v| v.to_str().ok())
        .and_then(|q| tauri::Url::parse(&format!("http://_/?{q}")).ok())
        .and_then(|url| url.query_pairs().find(|(k, _)| k == "name").map(|(_, v)| v.into_owned()))
        // Path separators would let a song title pick the directory.
        .map(|n| n.replace(['/', '\\', ':'], "_"))
        .filter(|n| !n.trim().is_empty())
        .unwrap_or_else(|| "lyrics".into());
    Ok((png, format!("{name}.png")))
}

/// Save the PNG wherever the user picks. `Ok(false)` means they cancelled.
#[cfg(not(mobile))]
#[tauri::command]
pub async fn share_save_image<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    request: Request<'_>,
) -> Result<bool, String> {
    use tauri_plugin_dialog::DialogExt;

    let (png, name) = png_and_name(&request)?;
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.dialog()
        .file()
        .set_file_name(&name)
        .add_filter("PNG", &["png"])
        .save_file(move |picked| {
            let _ = tx.send(picked);
        });
    let picked = rx.await.map_err(|_| "save dialog was dropped".to_string())?;
    let Some(path) = picked else { return Ok(false) };
    let path = path
        .into_path()
        .map_err(|e| format!("save dialog returned an unusable path: {e}"))?;
    std::fs::write(&path, png).map_err(|e| format!("write {}: {e}", path.display()))?;
    log::info!("[share] saved lyric card to {}", path.display());
    Ok(true)
}

/// The chooser reports `RESULT_CANCELED` for a dismissed sheet, and sharekit
/// turns that into this error. Dismissing a share sheet is not a failure.
#[cfg(mobile)]
fn dismissed_is_ok(result: tauri_plugin_sharekit::Result<()>) -> Result<(), String> {
    match result {
        Err(e) if e.to_string().contains("Share cancelled") => Ok(()),
        other => other.map_err(|e| e.to_string()),
    }
}

#[cfg(mobile)]
#[tauri::command]
pub async fn share_text<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    window: tauri::WebviewWindow<R>,
    text: String,
) -> Result<(), String> {
    use tauri_plugin_sharekit::{ShareExt, ShareTextOptions};

    dismissed_is_ok(
        app.share()
            .share_text(window, text, ShareTextOptions { mime_type: None, position: None })
            .await,
    )
}

#[cfg(mobile)]
#[tauri::command]
pub async fn share_image<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    window: tauri::WebviewWindow<R>,
    request: Request<'_>,
) -> Result<(), String> {
    use tauri::Manager;
    use tauri_plugin_sharekit::{ShareExt, ShareFileOptions};

    let (png, name) = png_and_name(&request)?;
    // sharekit copies the file to `cacheDir/<file name>` before granting it to
    // the receiver, so the source must not already sit at that exact path — the
    // copy would truncate it to nothing first.
    let dir = app.path().app_cache_dir().map_err(|e| e.to_string())?.join("share");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join(&name);
    std::fs::write(&path, png).map_err(|e| format!("write {}: {e}", path.display()))?;
    dismissed_is_ok(
        app.share()
            .share_file(
                window,
                path.to_string_lossy().into_owned(),
                ShareFileOptions {
                    mime_type: Some("image/png".into()),
                    title: Some(name),
                    position: None,
                },
            )
            .await,
    )
}
