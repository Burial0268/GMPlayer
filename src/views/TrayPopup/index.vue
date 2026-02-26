<template>
  <div class="tray-popup" @contextmenu.prevent>
    <div class="song-info">
      <img class="cover" :src="coverUrl || '/images/pic/default.png'" alt="cover" />
      <div class="text">
        <div class="title">{{ title || "GMPlayer" }}</div>
        <div class="artist">{{ artist || "" }}</div>
      </div>
    </div>
    <div class="controls">
      <button class="ctrl-btn" @click="prevTrack" :title="'Previous'">
        <svg viewBox="0 0 24 24" width="20" height="20" fill="currentColor">
          <path d="M6 6h2v12H6zm3.5 6l8.5 6V6z" />
        </svg>
      </button>
      <button class="ctrl-btn play" @click="playPause" :title="isPlaying ? 'Pause' : 'Play'">
        <svg v-if="isPlaying" viewBox="0 0 24 24" width="28" height="28" fill="currentColor">
          <path d="M6 19h4V5H6v14zm8-14v14h4V5h-4z" />
        </svg>
        <svg v-else viewBox="0 0 24 24" width="28" height="28" fill="currentColor">
          <path d="M8 5v14l11-7z" />
        </svg>
      </button>
      <button class="ctrl-btn" @click="nextTrack" :title="'Next'">
        <svg viewBox="0 0 24 24" width="20" height="20" fill="currentColor">
          <path d="M6 18l8.5-6L6 6v12zM16 6v12h2V6h-2z" />
        </svg>
      </button>
    </div>
    <button class="quit-btn" @click="quitApp" :title="'Quit'">
      <svg viewBox="0 0 24 24" width="14" height="14" fill="currentColor">
        <path
          d="M19 6.41L17.59 5 12 10.59 6.41 5 5 6.41 10.59 12 5 17.59 6.41 19 12 13.41 17.59 19 19 17.59 13.41 12z"
        />
      </svg>
    </button>
  </div>
</template>

<script setup lang="ts">
import { ref, onMounted, onUnmounted } from "vue";

const title = ref("");
const artist = ref("");
const coverUrl = ref("");
const isPlaying = ref(false);

const unlisteners: (() => void)[] = [];

const getTauri = () => window.__TAURI__;

onMounted(async () => {
  const tauri = getTauri();
  if (!tauri) return;

  // Listen for player state updates from main window
  const unlisten1 = await tauri.event.listen<{
    title: string;
    artist: string;
    coverUrl: string;
    isPlaying: boolean;
  }>("player-state-update", (event) => {
    title.value = event.payload.title;
    artist.value = event.payload.artist;
    coverUrl.value = event.payload.coverUrl;
    isPlaying.value = event.payload.isPlaying;
  });
  unlisteners.push(unlisten1);

  // Listen for popup opened event (triggers fresh state push from main)
  const unlisten2 = await tauri.event.listen("tray-popup-opened", () => {
    // Main window will push fresh state in response
  });
  unlisteners.push(unlisten2);
});

onUnmounted(() => {
  unlisteners.forEach((fn) => fn());
});

const playPause = () => {
  getTauri()?.event.emit("tray-play-pause", null);
};

const prevTrack = () => {
  getTauri()?.event.emit("tray-prev-track", null);
};

const nextTrack = () => {
  getTauri()?.event.emit("tray-next-track", null);
};

const quitApp = () => {
  getTauri()?.core.invoke("quit_app");
};
</script>

<style lang="scss" scoped>
.tray-popup {
  width: 320px;
  height: 180px;
  background: rgba(30, 30, 30, 0.85);
  backdrop-filter: blur(20px);
  -webkit-backdrop-filter: blur(20px);
  border-radius: 12px;
  border: 1px solid rgba(255, 255, 255, 0.08);
  color: #fff;
  font-family:
    "Segoe UI",
    system-ui,
    -apple-system,
    sans-serif;
  display: flex;
  flex-direction: column;
  padding: 16px;
  box-sizing: border-box;
  position: relative;
  overflow: hidden;
  user-select: none;
}

.song-info {
  display: flex;
  align-items: center;
  gap: 12px;
  flex: 1;
  min-height: 0;

  .cover {
    width: 64px;
    height: 64px;
    border-radius: 8px;
    object-fit: cover;
    flex-shrink: 0;
    background: rgba(255, 255, 255, 0.05);
  }

  .text {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 4px;

    .title {
      font-size: 14px;
      font-weight: 600;
      white-space: nowrap;
      overflow: hidden;
      text-overflow: ellipsis;
    }

    .artist {
      font-size: 12px;
      color: rgba(255, 255, 255, 0.6);
      white-space: nowrap;
      overflow: hidden;
      text-overflow: ellipsis;
    }
  }
}

.controls {
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 16px;
  padding-top: 12px;
}

.ctrl-btn {
  display: flex;
  align-items: center;
  justify-content: center;
  background: none;
  border: none;
  color: rgba(255, 255, 255, 0.85);
  cursor: pointer;
  padding: 8px;
  border-radius: 50%;
  transition: all 0.15s ease;

  &:hover {
    background: rgba(255, 255, 255, 0.1);
    color: #fff;
  }

  &:active {
    transform: scale(0.92);
  }

  &.play {
    background: rgba(255, 255, 255, 0.12);
    padding: 10px;

    &:hover {
      background: rgba(255, 255, 255, 0.2);
    }
  }
}

.quit-btn {
  position: absolute;
  top: 8px;
  right: 8px;
  display: flex;
  align-items: center;
  justify-content: center;
  background: none;
  border: none;
  color: rgba(255, 255, 255, 0.4);
  cursor: pointer;
  padding: 6px;
  border-radius: 50%;
  transition: all 0.15s ease;

  &:hover {
    background: rgba(255, 255, 255, 0.1);
    color: rgba(255, 255, 255, 0.8);
  }
}
</style>
