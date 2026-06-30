<template>
  <n-tooltip placement="right" :disabled="!collapsed" :delay="300">
    <template #trigger>
      <div :class="['sidebar-playlist-item', { collapsed }]" @click="$emit('navigate', id)">
        <span class="sidebar-playlist-cover-slot">
          <img
            class="sidebar-playlist-cover"
            :src="
              cover ? cover.replace(/^http:/, 'https:') + '?param=50y50' : '/images/pic/default.png'
            "
            alt="cover"
            loading="lazy"
          />
        </span>
        <span :class="['sidebar-playlist-name text-hidden', { hidden: collapsed }]">{{
          name
        }}</span>
      </div>
    </template>
    {{ name }}
  </n-tooltip>
</template>

<script setup>
import { NTooltip } from "naive-ui";

defineProps({
  id: { type: Number, required: true },
  cover: { type: String, default: "" },
  name: { type: String, required: true },
  collapsed: { type: Boolean, default: false },
});

defineEmits(["navigate"]);
</script>

<style lang="scss" scoped>
.sidebar-playlist-item {
  display: grid;
  grid-template-columns: var(--sidebar-item-slot, 40px) minmax(0, 1fr);
  align-items: center;
  width: 100%;
  min-height: 31px;
  padding: 0;
  border-radius: var(--radius-sm);
  cursor: pointer;
  transition:
    background-color 0.2s,
    width 0.3s ease;
  overflow: hidden;
  white-space: nowrap;

  &.collapsed {
    width: var(--sidebar-item-slot, 40px);
    min-width: var(--sidebar-item-slot, 40px);
    max-width: var(--sidebar-item-slot, 40px);
    min-height: 34px;
    margin-inline: auto;
    grid-template-columns: var(--sidebar-item-slot, 40px) 0;
  }

  &:hover {
    background-color: var(--sidebar-hover-bg, var(--n-border-color));
  }

  &:active {
    transform: scale(0.98);
  }
}

.sidebar-playlist-cover-slot {
  width: var(--sidebar-item-slot, 40px);
  min-width: var(--sidebar-item-slot, 40px);
  height: 31px;
  display: flex;
  align-items: center;
  justify-content: center;
}

.sidebar-playlist-cover {
  width: 22px;
  height: 22px;
  min-width: 22px;
  border-radius: var(--radius-sm);
  object-fit: cover;
  transition: border-radius 0.2s;

  .collapsed & {
    border-radius: var(--radius-pill);
  }
}

.sidebar-playlist-name {
  min-width: 0;
  padding-right: 10px;
  font-size: 12.5px;
  line-height: 18px;
  color: var(--sidebar-text, var(--n-text-color));
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  opacity: 1;
  transform: translateX(0);
  transition:
    opacity 0.2s ease 0.1s,
    transform 0.3s ease;

  &.hidden {
    opacity: 0;
    transform: translateX(-4px);
    transition:
      opacity 0.1s ease,
      transform 0.3s ease;
  }
}
</style>
