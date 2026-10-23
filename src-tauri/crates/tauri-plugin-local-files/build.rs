/// Every command is reached from Rust through `run_mobile_plugin`, never from
/// JS, so none of these names appear in a `capabilities/*.json`. They are still
/// declared because `tauri_plugin::Builder` generates the per-command
/// permission files from this list, and a plugin with no permission set at all
/// is rejected at build time.
const COMMANDS: &[&str] = &[
    "pick_tree",
    "pick_writable_tree",
    "pick_files",
    "enumerate_tree",
    "cancel_enumerate",
    "open_fd",
    "read_bytes",
    "pick_text_document",
    "create_document",
    "find_document",
    "open_write_fd",
    "delete_document",
    "rename_document",
    "document_exists",
    "list_persisted",
    "release_uri",
];

fn main() {
    tauri_plugin::Builder::new(COMMANDS)
        .android_path("android")
        .build();
}
