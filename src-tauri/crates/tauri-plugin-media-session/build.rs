const COMMANDS: &[&str] = &[
    "initialize",
    "update_state",
    "update_timeline",
    "clear",
    "register_listener",
    "remove_listener",
];

fn main() {
    tauri_plugin::Builder::new(COMMANDS)
        .android_path("android")
        .ios_path("ios")
        .build();
}
