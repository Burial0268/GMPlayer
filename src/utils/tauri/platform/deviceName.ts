import { invoke } from "@tauri-apps/api/core";
import { isTauri } from "../core/runtime";
import { isMobile } from "./mobile";

/** The name the OS shows for this device, or null outside Tauri or when it cannot tell. */
export async function getDeviceName(): Promise<string | null> {
  if (!isTauri()) return null;
  try {
    if (await isMobile()) {
      const { name } = await invoke<{ name: string }>("plugin:orientation|deviceName");
      return name || null;
    }
    return await invoke<string | null>("device_name");
  } catch (err) {
    console.warn("[DeviceName] Failed to read the device name:", err);
    return null;
  }
}
