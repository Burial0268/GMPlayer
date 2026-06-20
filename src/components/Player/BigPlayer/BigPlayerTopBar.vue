<template>
  <div class="icon-menu">
    <div class="menu-left">
      <div v-if="showLyricSetting" class="icon">
        <n-icon
          class="setting"
          size="30"
          :component="SettingsRound"
          @click="$emit('openSettings')"
        />
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { SettingsRound } from "@vicons/material";

defineProps<{
  showLyricSetting: boolean;
}>();

defineEmits<{
  openSettings: [];
}>();
</script>

<style lang="scss" scoped>
.icon-menu {
  padding: 20px;
  // On Tauri mobile (tablet ≥ 768px), push icons below the status-bar / notch.
  // --app-safe-area-top is env(safe-area-inset-top) on Tauri mobile, 0px everywhere else.
  padding-top: calc(var(--app-safe-area-top, 0px) + 20px);
  width: 100%;
  height: calc(80px + var(--app-safe-area-top, 0px));
  position: absolute;
  top: 0;
  left: 0;
  display: flex;
  mix-blend-mode: plus-lighter;
  align-items: center;
  justify-content: space-between;
  z-index: 5;
  box-sizing: border-box;
  pointer-events: none;

  @media (max-width: 768px) {
    display: none;
  }

  .menu-left {
    position: relative;
    z-index: 1;
    display: flex;
    align-items: center;
    pointer-events: auto;

    .icon {
      width: 40px;
      height: 40px;
      display: flex;
      align-items: center;
      justify-content: center;
      font-size: 40px;
      opacity: 0.3;
      border-radius: 8px;
      transition: all 0.3s cubic-bezier(0.34, 1.56, 0.64, 1);
      cursor: pointer;
      will-change: transform, opacity, background-color;

      &:hover {
        background-color: #ffffff20;
        transform: scale(1.05);
        opacity: 1;
      }

      &:active {
        transform: scale(1);
      }

      .setting {
        @media (max-width: 768px) {
          display: none;
        }
      }
    }
  }

  .menu-left {
    .icon + .icon {
      margin-left: 8px;
    }
  }
}
</style>
