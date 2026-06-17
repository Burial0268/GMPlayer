<template>
  <n-tooltip placement="right" :disabled="!collapsed" :delay="300">
    <template #trigger>
      <router-link :to="to" :class="['sidebar-item', { collapsed }]" @click="$emit('navigate')">
        <span class="sidebar-item-icon-slot">
          <n-icon class="sidebar-item-icon" :size="16" :component="icon" />
        </span>
        <span :class="['sidebar-item-label', { hidden: collapsed }]">{{ label }}</span>
      </router-link>
    </template>
    {{ label }}
  </n-tooltip>
</template>

<script setup>
import { NIcon, NTooltip } from "naive-ui";

defineProps({
  to: { type: [String, Object], required: true },
  icon: { type: Object, required: true },
  label: { type: String, required: true },
  collapsed: { type: Boolean, default: false },
});

defineEmits(["navigate"]);
</script>

<style lang="scss" scoped>
.sidebar-item {
  display: grid;
  grid-template-columns: var(--sidebar-item-slot, 40px) minmax(0, 1fr);
  align-items: center;
  width: 100%;
  min-height: 32px;
  padding: 0;
  border-radius: var(--radius-sm);
  text-decoration: none;
  color: var(--sidebar-text, var(--n-text-color));
  transition:
    background-color var(--duration-150) var(--ease-out),
    color var(--duration-150) var(--ease-out),
    width var(--duration-300) var(--ease-out);
  cursor: pointer;
  white-space: nowrap;
  overflow: hidden;

  &.collapsed {
    width: var(--sidebar-item-slot, 40px);
    min-height: 34px;
    margin-inline: auto;
  }

  &:hover {
    background-color: var(--sidebar-hover-bg, var(--n-border-color));
  }

  &:active {
    transform: scale(0.97);
    transition: transform var(--duration-150) var(--ease-out);
  }

  &.router-link-active {
    color: var(--sidebar-accent, var(--main-color));
    background-color: var(
      --sidebar-active-bg,
      color-mix(in srgb, var(--main-color) 14%, transparent)
    );
    font-weight: 600;

    .sidebar-item-icon {
      color: var(--sidebar-accent, var(--main-color));
    }
  }
}

.sidebar-item-icon-slot {
  width: var(--sidebar-item-slot, 40px);
  min-width: var(--sidebar-item-slot, 40px);
  height: 32px;
  display: flex;
  align-items: center;
  justify-content: center;
}

.sidebar-item-icon {
  transition:
    color var(--duration-150) var(--ease-out),
    transform var(--duration-300) var(--ease-out);
}

.sidebar-item-label {
  min-width: 0;
  padding-right: 10px;
  font-size: 12.5px;
  line-height: 18px;
  overflow: hidden;
  text-overflow: ellipsis;
  text-align: left;
  opacity: 1;
  transform: translateX(0);
  transition:
    opacity var(--duration-200) var(--ease-out) var(--duration-100),
    transform var(--duration-300) var(--ease-out);

  &.hidden {
    opacity: 0;
    transform: translateX(-4px);
    transition:
      opacity var(--duration-100) var(--ease-out),
      transform var(--duration-300) var(--ease-out);
  }
}
</style>
