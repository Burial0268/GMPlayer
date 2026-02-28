<template>
  <n-popover v-if="listenTogether.isInRoom" trigger="hover" :keep-alive-on-hover="false">
    <template #trigger>
      <div class="listen-together-status" @click.stop="$emit('click')">
        <n-badge :value="listenTogether.onlineCount" :max="99" type="success">
          <n-icon size="22" :component="PeopleOutline" />
        </n-badge>
        <span v-if="!isMobile" class="status-text">
          {{ listenTogether.isHost ? t("other.listenTogether.host") : t("other.listenTogether.guest") }}
        </span>
      </div>
    </template>
    <div class="popover-content">
      <div class="room-info-line">
        <span class="label">{{ t("other.listenTogether.roomId") }}:</span>
        <code>{{ listenTogether.roomId }}</code>
      </div>
      <div class="users-count">
        {{ t("other.listenTogether.onlineUsers") }}: {{ listenTogether.onlineCount }}
      </div>
      <div class="sync-status" v-if="listenTogether.syncStatus === 'error'">
        <n-text type="error">{{ t("other.listenTogether.syncError") }}</n-text>
      </div>
    </div>
  </n-popover>
  <n-popover v-else trigger="hover" :keep-alive-on-hover="false">
    <template #trigger>
      <div class="listen-together-status inactive" @click.stop="$emit('click')">
        <n-icon size="22" :component="PeopleOutline" />
      </div>
    </template>
    {{ t("setting.listenTogether") }}
  </n-popover>
</template>

<script setup>
import { useI18n } from "vue-i18n";
import { listenTogetherStore } from "@/store";
import { PeopleOutline } from "@vicons/ionicons5";
import { useResponsiveLayout } from "@/composables/useResponsiveLayout";

const { t } = useI18n();
const listenTogether = listenTogetherStore();
const { isMobile } = useResponsiveLayout();

defineEmits(["click"]);
</script>

<style scoped lang="scss">
.listen-together-status {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 4px 8px;
  border-radius: 6px;
  cursor: pointer;
  transition: all 0.2s ease;
  color: var(--main-color);
  background-color: var(--main-color-hover);

  &:hover {
    background-color: var(--main-color);
    color: var(--main-second-color);
  }

  &.inactive {
    color: var(--n-text-color-3);
    background-color: transparent;

    &:hover {
      background-color: var(--n-action-color);
      color: var(--main-color);
    }
  }

  .status-text {
    font-size: 12px;
    font-weight: 500;
  }

  :deep(.n-badge) {
    .n-badge-sup {
      height: 16px;
      line-height: 16px;
      min-width: 16px;
      padding: 0 4px;
      font-size: 10px;
    }
  }
}

.popover-content {
  .room-info-line {
    display: flex;
    align-items: center;
    gap: 6px;
    margin-bottom: 4px;

    .label {
      color: var(--n-text-color-3);
    }

    code {
      font-family: monospace;
      color: var(--main-color);
      background-color: var(--n-action-color);
      padding: 2px 6px;
      border-radius: 4px;
    }
  }

  .users-count {
    font-size: 12px;
    color: var(--n-text-color-2);
  }

  .sync-status {
    margin-top: 4px;
    font-size: 11px;
  }
}
</style>
