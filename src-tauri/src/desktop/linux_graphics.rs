use std::env;

const GRAPHICS_MODE_ENV: &str = "GMPLAYER_LINUX_GRAPHICS";

pub fn configure_webkit_gtk_backend() {
    let mode = env::var(GRAPHICS_MODE_ENV)
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();

    match mode.as_str() {
        "" | "auto" => configure_auto(),
        "default" | "system" | "none" => {}
        "wayland" => {
            set_env("GDK_BACKEND", "wayland,x11");
            enable_accelerated_compositing_hint();
        }
        "x11" | "xwayland" => {
            set_env("GDK_BACKEND", "x11");
            set_env("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
            enable_accelerated_compositing_hint();
        }
        "safe" | "safe-wayland" | "dmabuf-off" | "disable-dmabuf" => {
            set_env("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
            enable_accelerated_compositing_hint();
        }
        "software" | "cpu" => {
            set_env("WEBKIT_DISABLE_COMPOSITING_MODE", "1");
        }
        other => {
            eprintln!(
                "Unknown {GRAPHICS_MODE_ENV}='{other}', falling back to automatic Linux graphics policy"
            );
            configure_auto();
        }
    }
}

fn configure_auto() {
    if !is_wayland_session() {
        return;
    }

    // WebKitGTK's DMABUF renderer is still fragile on parts of the Wayland
    // stack, especially with proprietary NVIDIA drivers and mixed-GPU systems.
    // Disabling only that renderer keeps accelerated compositing available via
    // the older GL path while avoiding blank/black webviews on affected setups.
    set_if_unset("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
    enable_accelerated_compositing_hint();
}

fn enable_accelerated_compositing_hint() {
    if env::var_os("WEBKIT_DISABLE_COMPOSITING_MODE").is_none() {
        set_if_unset("WEBKIT_FORCE_COMPOSITING_MODE", "1");
    }
}

fn is_wayland_session() -> bool {
    env::var_os("WAYLAND_DISPLAY").is_some()
        || env::var("XDG_SESSION_TYPE")
            .map(|value| value.eq_ignore_ascii_case("wayland"))
            .unwrap_or(false)
}

fn set_if_unset(key: &str, value: &str) {
    if env::var_os(key).is_none() {
        set_env(key, value);
    }
}

fn set_env(key: &str, value: &str) {
    env::set_var(key, value);
}
