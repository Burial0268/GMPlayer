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
    <div class="menu-right">
      <div class="icon">
        <n-icon class="screenfull" :component="screenfullIcon" @click="$emit('toggleFullscreen')" />
      </div>
      <div class="icon">
        <n-icon class="close" :component="KeyboardArrowDownFilled" @click="$emit('close')" />
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { KeyboardArrowDownFilled, SettingsRound } from "@vicons/material";
import type { Component } from "vue";

defineProps<{
  showLyricSetting: boolean;
  screenfullIcon: Component;
}>();

defineEmits<{
  openSettings: [];
  toggleFullscreen: [];
  close: [];
}>();
</script>

<style lang="scss" scoped>
.icon-menu {
  padding: 20px;
  width: 100%;
  height: 80px;
  position: absolute;
  top: 0;
  left: 0;
  display: flex;
  mix-blend-mode: plus-lighter;
  align-items: center;
  justify-content: space-between;
  z-index: 5;
  box-sizing: border-box;

  @media (max-width: 768px) {
    display: none;
  }

  .menu-left,
  .menu-right {
    display: flex;
    align-items: center;

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

      .screenfull,
      .setting {
        @media (max-width: 768px) {
          display: none;
        }
      }
    }
  }

  .menu-right {
    .icon {
      margin-left: 12px;
    }
  }
}
</style>
