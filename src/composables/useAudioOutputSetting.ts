import { watch } from "vue";
import { settingStore } from "@/store";
import { audioSendMsg } from "@/utils/tauri/audio/bridge";

/**
 * Keeps the native output on the device the user picked (`setting.audioOutputDevice`).
 *
 * The backend starts on the system default and does not remember a pick across launches, so a
 * stored name is sent once at startup. An empty one is not: `SetAudioOutput` forces an output
 * refresh, which a backend already on the default does not need.
 */
export function useAudioOutputSetting() {
  const setting = settingStore();
  watch(
    () => setting.audioOutputDevice,
    (name, previous) => {
      if (previous === undefined && !name) return;
      void audioSendMsg({ type: "setAudioOutput", name });
    },
    { immediate: true },
  );
}
