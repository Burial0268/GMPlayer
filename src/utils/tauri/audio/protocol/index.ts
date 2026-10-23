/**
 * The wire protocol shared with `src-tauri/crates/audio-backend`.
 *
 * Types only — no runtime code, so importing this from either the Tauri or the
 * WASM transport costs nothing at runtime.
 */
export type * from "./manifest";
export type * from "./messages";
export type * from "./events";
