<template>
  <n-modal
    v-model:show="showModal"
    preset="card"
    :title="getModalTitle"
    class="lt-modal"
    :style="{ width: isMobile ? '92%' : '420px' }"
    :bordered="false"
    transform-origin="center"
  >
    <div class="lt-body">
      <!-- ══════════════════════════════════════════════════
           NOT IN ROOM — Create / Join tabs
           ══════════════════════════════════════════════════ -->
      <template v-if="!listenTogether.isInRoom">
        <n-tabs type="segment" v-model:value="activeTab" animated>
          <!-- ─── Create tab ─────────────────────────────── -->
          <n-tab-pane name="create" :tab="t('other.listenTogether.createRoom')">
            <div class="tab-pane">
              <template v-if="!userStore.userLogin">
                <n-alert type="warning" :bordered="false" class="auth-alert">
                  {{ t("other.listenTogether.needLogin") }}
                </n-alert>
              </template>

              <template v-else-if="!musicStore.getPlaySongData?.id">
                <n-empty
                  :description="t('other.listenTogether.noSong')"
                  class="empty-hint"
                  size="medium"
                />
              </template>

              <template v-else>
                <!-- Current song preview -->
                <div class="song-preview-card">
                  <div class="song-cover-wrap">
                    <img
                      :src="coverUrl(musicStore.getPlaySongData.album?.picUrl, 120)"
                      class="song-cover-img"
                      alt="cover"
                    />
                    <!-- Pulse ring when playing -->
                    <div v-if="musicStore.getPlayState" class="cover-pulse" />
                  </div>
                  <div class="song-preview-info">
                    <div class="preview-name text-hidden">
                      {{ musicStore.getPlaySongData.name }}
                    </div>
                    <AllArtists
                      :artistsData="musicStore.getPlaySongData.artist"
                      class="preview-artists"
                    />
                  </div>
                </div>

                <p class="create-hint">{{ t("other.listenTogether.createHint") }}</p>

                <n-button
                  type="primary"
                  size="large"
                  block
                  strong
                  :loading="isLoading"
                  @click="handleCreateRoom"
                >
                  <template #icon>
                    <n-icon :component="PeopleOutline" />
                  </template>
                  {{ t("other.listenTogether.createRoom") }}
                </n-button>
              </template>
            </div>
          </n-tab-pane>

          <!-- ─── Join tab ──────────────────────────────── -->
          <n-tab-pane name="join" :tab="t('other.listenTogether.joinRoom')">
            <div class="tab-pane">
              <template v-if="!userStore.userLogin">
                <n-alert type="warning" :bordered="false" class="auth-alert">
                  {{ t("other.listenTogether.needLogin") }}
                </n-alert>
              </template>

              <template v-else>
                <n-input
                  v-model:value="joinRoomId"
                  :placeholder="t('other.listenTogether.roomIdPlaceholder')"
                  size="large"
                  clearable
                  maxlength="20"
                  @keydown.enter="handleJoinRoom"
                >
                  <template #prefix>
                    <n-icon :component="KeyOutline" />
                  </template>
                </n-input>

                <n-button
                  type="primary"
                  size="large"
                  block
                  strong
                  :loading="isLoading"
                  :disabled="!joinRoomId.trim()"
                  @click="handleJoinRoom"
                >
                  {{ t("other.listenTogether.joinRoom") }}
                </n-button>
              </template>
            </div>
          </n-tab-pane>
        </n-tabs>
      </template>

      <!-- ══════════════════════════════════════════════════
           IN ROOM
           ══════════════════════════════════════════════════ -->
      <template v-else>
        <div class="room-layout">
          <!-- ─── Room banner ─────────────────────────────── -->
          <div class="room-banner">
            <div class="banner-meta">
              <!-- Role badge -->
              <n-tag
                :type="listenTogether.isHost ? 'primary' : 'success'"
                size="small"
                round
                :bordered="false"
                class="role-tag"
              >
                <template #icon>
                  <n-icon
                    :component="listenTogether.isHost ? StarSharp : PersonOutline"
                    size="12"
                  />
                </template>
                {{
                  listenTogether.isHost
                    ? t("other.listenTogether.host")
                    : t("other.listenTogether.guest")
                }}
              </n-tag>

              <!-- Room ID row -->
              <div class="room-id-row">
                <span class="room-id-label">{{ t("other.listenTogether.roomId") }}</span>
                <code class="room-id-code">{{ listenTogether.roomId }}</code>
                <n-button text size="tiny" :focusable="false" class="icon-btn" @click="copyRoomId">
                  <template #icon>
                    <n-icon :component="CopyOutline" size="14" />
                  </template>
                </n-button>
              </div>
            </div>

            <!-- Share link button -->
            <n-button
              size="small"
              secondary
              :focusable="false"
              class="share-btn"
              @click="copyShareLink"
            >
              <template #icon>
                <n-icon :component="ShareSocialOutline" />
              </template>
              {{ t("other.listenTogether.copyLink") }}
            </n-button>
          </div>

          <!-- ─── Sync status bar ─────────────────────────── -->
          <!-- Always visible for guests; visible for hosts only on error -->
          <Transition name="lt-slide">
            <div v-if="showSyncBar" :class="['sync-bar', `sync-bar--${listenTogether.syncStatus}`]">
              <span class="sync-dot" />
              <span class="sync-label">{{ syncStatusText }}</span>
            </div>
          </Transition>

          <!-- ─── Online users ────────────────────────────── -->
          <div class="section">
            <div class="section-header">
              <span class="section-title">{{ t("other.listenTogether.onlineUsers") }}</span>
              <n-badge
                :value="listenTogether.onlineCount"
                :max="99"
                type="success"
                :show-zero="true"
                class="count-badge"
              />
            </div>

            <div class="users-list">
              <!-- Host row (always first) -->
              <div
                v-if="listenTogether.hostInfo"
                :class="['user-row', { 'user-row--self': isSelf(listenTogether.hostInfo.userId) }]"
              >
                <div class="user-avatar" :style="avatarBg(listenTogether.hostInfo.userId)">
                  <img
                    :src="listenTogether.hostInfo.avatarUrl + '?param=60y60'"
                    class="avatar-img"
                    @error="hideImg"
                    alt=""
                  />
                  <span class="avatar-letter">
                    {{ firstLetter(listenTogether.hostInfo.nickname) }}
                  </span>
                </div>
                <span class="user-nickname text-hidden">{{
                  listenTogether.hostInfo.nickname
                }}</span>
                <div class="user-tags">
                  <n-tag
                    v-if="isSelf(listenTogether.hostInfo.userId)"
                    size="tiny"
                    type="info"
                    :bordered="false"
                    round
                  >
                    {{ t("other.listenTogether.me") }}
                  </n-tag>
                  <n-tag size="tiny" type="primary" :bordered="false" round>
                    <template #icon>
                      <n-icon :component="StarSharp" size="10" />
                    </template>
                    {{ t("other.listenTogether.host") }}
                  </n-tag>
                </div>
              </div>

              <!-- Guest rows -->
              <div
                v-for="user in listenTogether.users"
                :key="user.userId"
                :class="['user-row', { 'user-row--self': isSelf(user.userId) }]"
              >
                <div class="user-avatar" :style="avatarBg(user.userId)">
                  <img
                    :src="user.avatarUrl + '?param=60y60'"
                    class="avatar-img"
                    @error="hideImg"
                    alt=""
                  />
                  <span class="avatar-letter">{{ firstLetter(user.nickname) }}</span>
                </div>
                <span class="user-nickname text-hidden">{{ user.nickname }}</span>
                <n-tag v-if="isSelf(user.userId)" size="tiny" type="info" :bordered="false" round>
                  {{ t("other.listenTogether.me") }}
                </n-tag>
              </div>

              <!-- Empty -->
              <div
                v-if="!listenTogether.hostInfo && !listenTogether.users.length"
                class="users-empty"
              >
                {{ t("other.listenTogether.waitingForHost") }}
              </div>
            </div>
          </div>

          <!-- ─── Now playing ─────────────────────────────── -->
          <div v-if="musicStore.getPlaySongData" class="section now-playing-section">
            <div class="section-header">
              <span class="section-title">{{ t("general.name.play") }}</span>
              <Transition name="lt-fade">
                <div v-if="musicStore.getPlayState" class="playing-badge">
                  <span class="playing-dot" />
                  <span>{{ t("other.listenTogether.playing") }}</span>
                </div>
              </Transition>
            </div>

            <div class="now-playing-card">
              <div :class="['np-cover', { 'np-cover--spinning': musicStore.getPlayState }]">
                <img
                  :src="coverUrl(musicStore.getPlaySongData.album?.picUrl, 100)"
                  class="np-cover-img"
                  alt="cover"
                />
              </div>
              <div class="np-info">
                <div class="np-name text-hidden">{{ musicStore.getPlaySongData.name }}</div>
                <AllArtists :artistsData="musicStore.getPlaySongData.artist" class="np-artists" />
              </div>
            </div>
          </div>

          <!-- ─── Leave / close button ────────────────────── -->
          <n-button
            :type="listenTogether.isHost ? 'error' : 'default'"
            size="large"
            block
            :ghost="listenTogether.isHost"
            :secondary="!listenTogether.isHost"
            :loading="isLoading"
            :focusable="false"
            class="action-btn"
            @click="handleLeaveRoom"
          >
            {{
              listenTogether.isHost
                ? t("other.listenTogether.closeRoom")
                : t("other.listenTogether.leaveRoom")
            }}
          </n-button>
        </div>
      </template>
    </div>
  </n-modal>
</template>

<script setup>
import { ref, computed } from "vue";
import { useI18n } from "vue-i18n";
import { useMusicDataStore, useUserDataStore, useListenTogetherStore } from "@/store";
import {
  CopyOutline,
  ShareSocialOutline,
  PeopleOutline,
  PersonOutline,
  StarSharp,
  KeyOutline,
} from "@vicons/ionicons5";
import AllArtists from "@/components/DataList/AllArtists.vue";
import { useResponsiveLayout } from "@/composables/useResponsiveLayout";
import { coverUrl } from "@/utils/coverUrl";

// ── i18n / stores ──────────────────────────────────────────────────────────
const { t } = useI18n();
const musicStore = useMusicDataStore();
const userStore = useUserDataStore();
const listenTogether = useListenTogetherStore();
const { isMobile } = useResponsiveLayout();

// ── Props / emits ──────────────────────────────────────────────────────────
const props = defineProps({
  show: { type: Boolean, default: false },
});
const emit = defineEmits(["update:show"]);

const showModal = computed({
  get: () => props.show,
  set: (val) => emit("update:show", val),
});

// ── Modal title ────────────────────────────────────────────────────────────
const getModalTitle = computed(() => {
  if (listenTogether.isInRoom) {
    return listenTogether.isHost
      ? t("other.listenTogether.titleHost")
      : t("other.listenTogether.titleGuest");
  }
  return t("other.listenTogether.title");
});

// ── Tab / input state ──────────────────────────────────────────────────────
const activeTab = ref("create");
const joinRoomId = ref("");
const isLoading = ref(false);

// ── Sync status ────────────────────────────────────────────────────────────
/** Show the sync bar for guests always, for hosts only when there's an error. */
const showSyncBar = computed(() => listenTogether.isGuest || listenTogether.syncStatus === "error");

const syncStatusText = computed(() => {
  switch (listenTogether.syncStatus) {
    case "syncing":
      return t("other.listenTogether.syncing");
    case "error":
      return t("other.listenTogether.syncError");
    default:
      return t("other.listenTogether.synced");
  }
});

// ── Avatar helpers ─────────────────────────────────────────────────────────
// A set of muted, theme-neutral hues used for generated avatars.
const HUE_STEP = 137.508; // golden angle — ensures good hue distribution

function avatarBg(userId) {
  const hue = Math.round((userId * HUE_STEP) % 360);
  // Use lower lightness so white text stays readable on both themes.
  return { backgroundColor: `hsl(${hue}, 55%, 50%)` };
}

function firstLetter(nickname) {
  return (nickname || "?")[0].toUpperCase();
}

/**
 * When an avatar image fails to load, hide it so the CSS letter-initial
 * behind it becomes visible (the letter is always rendered in the DOM but
 * sits below the <img> in z-order).
 */
function hideImg(event) {
  event.target.style.display = "none";
}

function isSelf(userId) {
  return userId === userStore.userData?.userId;
}

// ── Action handlers ────────────────────────────────────────────────────────
async function handleCreateRoom() {
  if (isLoading.value) return;
  isLoading.value = true;
  await listenTogether.createRoom();
  isLoading.value = false;
}

async function handleJoinRoom() {
  if (isLoading.value || !joinRoomId.value.trim()) return;
  isLoading.value = true;
  const ok = await listenTogether.joinRoom(joinRoomId.value.trim());
  isLoading.value = false;
  if (ok) joinRoomId.value = "";
}

async function handleLeaveRoom() {
  if (isLoading.value) return;
  isLoading.value = true;
  await listenTogether.leaveRoom();
  isLoading.value = false;
}

async function copyRoomId() {
  if (!listenTogether.roomId) return;
  try {
    await navigator.clipboard.writeText(listenTogether.roomId);
    $message.success(t("other.listenTogether.copySuccess"));
  } catch {
    $message.error(t("other.listenTogether.copyFailed"));
  }
}

async function copyShareLink() {
  const link = listenTogether.shareLink;
  if (!link) return;
  try {
    await navigator.clipboard.writeText(link);
    $message.success(t("other.listenTogether.copySuccess"));
  } catch {
    $message.error(t("other.listenTogether.copyFailed"));
  }
}
</script>

<style scoped lang="scss">
// ─── Card shell ─────────────────────────────────────────────────────────────
.lt-modal {
  :deep(.n-card__content) {
    padding: 16px;
  }
}

.lt-body {
  min-height: 220px;
}

// ─── Pre-join tab pane ───────────────────────────────────────────────────────
.tab-pane {
  display: flex;
  flex-direction: column;
  gap: 14px;
  padding-top: 18px;

  .auth-alert {
    border-radius: var(--radius-md);
  }

  .empty-hint {
    padding: 24px 0;
  }
}

// ─── Song preview card (Create tab) ─────────────────────────────────────────
.song-preview-card {
  display: flex;
  align-items: center;
  gap: 14px;
  padding: 12px 14px;
  background-color: var(--n-action-color);
  border-radius: var(--radius-lg);
  border: 1px solid var(--n-divider-color);

  .song-cover-wrap {
    position: relative;
    flex-shrink: 0;
    width: 56px;
    height: 56px;
    border-radius: var(--radius-md);
    overflow: visible;

    .song-cover-img {
      width: 56px;
      height: 56px;
      border-radius: var(--radius-md);
      object-fit: cover;
      display: block;
    }

    // Animated pulse ring when song is playing
    .cover-pulse {
      position: absolute;
      inset: -4px;
      border-radius: 12px;
      border: 2px solid var(--main-color);
      opacity: 0.7;
      animation: pulse-ring 2s ease-in-out infinite;
      pointer-events: none;
    }
  }

  .song-preview-info {
    flex: 1;
    min-width: 0;

    .preview-name {
      font-size: 14px;
      font-weight: 600;
      line-height: 1.4;
      margin-bottom: 4px;
    }

    :deep(.preview-artists .artists) {
      font-size: 12px;
      color: var(--n-text-color-3);
    }
  }
}

.create-hint {
  margin: 0;
  font-size: 12px;
  color: var(--n-text-color-3);
  text-align: center;
}

// ─── In-room layout ──────────────────────────────────────────────────────────
.room-layout {
  display: flex;
  flex-direction: column;
  gap: 16px;
}

// ─── Room banner ─────────────────────────────────────────────────────────────
.room-banner {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 12px;
  padding: 14px 16px;
  border-radius: var(--radius-panel);
  background: linear-gradient(
    135deg,
    color-mix(in srgb, var(--main-color) 12%, transparent),
    color-mix(in srgb, var(--main-color) 6%, transparent)
  );
  border: 1px solid color-mix(in srgb, var(--main-color) 20%, transparent);

  .banner-meta {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .role-tag {
    align-self: flex-start;
    font-weight: 600;
  }

  .room-id-row {
    display: flex;
    align-items: center;
    gap: 6px;

    .room-id-label {
      font-size: 11px;
      color: var(--n-text-color-3);
      white-space: nowrap;
    }

    .room-id-code {
      font-family: "SF Mono", "Fira Code", monospace;
      font-size: 17px;
      font-weight: 700;
      letter-spacing: 1px;
      color: var(--main-color);
      line-height: 1;
    }

    .icon-btn {
      color: var(--n-text-color-3);
      transition: color var(--duration-200) var(--ease-out);

      &:hover {
        color: var(--main-color);
      }
    }
  }

  .share-btn {
    flex-shrink: 0;
    margin-top: 2px;
  }
}

// ─── Sync status bar ─────────────────────────────────────────────────────────
.sync-bar {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 7px 12px;
  border-radius: var(--radius-md);
  font-size: 12px;
  font-weight: 500;
  transition: background-color var(--duration-300) var(--ease-out);

  // idle / synced
  &.sync-bar--idle {
    background-color: color-mix(in srgb, #18a058 10%, transparent);
    color: #18a058;

    .sync-dot {
      background-color: #18a058;
    }
  }

  // syncing
  &.sync-bar--syncing {
    background-color: color-mix(in srgb, #f0a020 10%, transparent);
    color: #d08010;

    .sync-dot {
      background-color: #f0a020;
      animation: sync-pulse 1s ease-in-out infinite;
    }
  }

  // error
  &.sync-bar--error {
    background-color: color-mix(in srgb, #d03050 10%, transparent);
    color: #d03050;

    .sync-dot {
      background-color: #d03050;
      animation: sync-blink 0.8s step-start infinite;
    }
  }

  .sync-dot {
    flex-shrink: 0;
    width: 7px;
    height: 7px;
    border-radius: 50%;
  }

  .sync-label {
    line-height: 1;
  }
}

// ─── Generic section ─────────────────────────────────────────────────────────
.section {
  display: flex;
  flex-direction: column;
  gap: 10px;
}

.section-header {
  display: flex;
  align-items: center;
  justify-content: space-between;

  .section-title {
    font-size: 11px;
    font-weight: 700;
    text-transform: uppercase;
    letter-spacing: 0.8px;
    color: var(--n-text-color-3);
  }

  .count-badge :deep(.n-badge-sup) {
    height: 18px;
    line-height: 18px;
    min-width: 18px;
    padding: 0 5px;
    font-size: 10px;
    border-radius: 9px;
  }
}

// ─── Users list ───────────────────────────────────────────────────────────────
.users-list {
  display: flex;
  flex-direction: column;
  gap: 2px;
  max-height: 168px;
  overflow-y: auto;
  // Hide scrollbar on non-hover
  scrollbar-width: thin;
  scrollbar-color: transparent transparent;

  &:hover {
    scrollbar-color: var(--n-scrollbar-color) transparent;
  }
}

.user-row {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 7px 10px;
  border-radius: var(--radius-md);
  transition: background-color var(--duration-150) var(--ease-out);

  &:hover {
    background-color: var(--n-action-color);
  }

  &.user-row--self {
    background-color: color-mix(in srgb, var(--main-color) 7%, transparent);

    &:hover {
      background-color: color-mix(in srgb, var(--main-color) 12%, transparent);
    }
  }
}

// Avatar: letter sits behind img; when img is hidden (onerror), letter shows.
.user-avatar {
  position: relative;
  flex-shrink: 0;
  width: 34px;
  height: 34px;
  border-radius: 50%;
  display: flex;
  align-items: center;
  justify-content: center;
  overflow: hidden;

  .avatar-img {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    object-fit: cover;
    border-radius: 50%;
    z-index: 1;
  }

  .avatar-letter {
    font-size: 14px;
    font-weight: 700;
    color: #fff;
    user-select: none;
    z-index: 0;
  }
}

.user-nickname {
  flex: 1;
  font-size: 13.5px;
  min-width: 0;
}

.user-tags {
  display: flex;
  align-items: center;
  gap: 4px;
  flex-shrink: 0;
}

.users-empty {
  padding: 12px 10px;
  font-size: 12px;
  color: var(--n-text-color-3);
  text-align: center;
}

// ─── Now playing section ──────────────────────────────────────────────────────
.now-playing-section {
  padding: 12px 14px;
  background-color: var(--n-action-color);
  border-radius: var(--radius-lg);
  border: 1px solid var(--n-divider-color);
}

.playing-badge {
  display: flex;
  align-items: center;
  gap: 5px;
  font-size: 11px;
  font-weight: 600;
  color: #18a058;

  .playing-dot {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background-color: #18a058;
    animation: pulse-ring 1.4s ease-in-out infinite;
    flex-shrink: 0;
  }
}

.now-playing-card {
  display: flex;
  align-items: center;
  gap: 12px;
}

.np-cover {
  flex-shrink: 0;
  width: 48px;
  height: 48px;
  border-radius: var(--radius-md);
  overflow: hidden;
  box-shadow: 0 3px 10px rgba(0, 0, 0, 0.18);
  transition: border-radius var(--duration-400) var(--ease-out);

  &.np-cover--spinning {
    border-radius: 50%;
    animation: spin-slow 12s linear infinite;
  }

  .np-cover-img {
    width: 100%;
    height: 100%;
    object-fit: cover;
    display: block;
  }
}

.np-info {
  flex: 1;
  min-width: 0;

  .np-name {
    font-size: 13.5px;
    font-weight: 600;
    line-height: 1.4;
    margin-bottom: 3px;
  }

  :deep(.np-artists .artists) {
    font-size: 11.5px;
    color: var(--n-text-color-3);
  }
}

// ─── Action button ────────────────────────────────────────────────────────────
.action-btn {
  margin-top: 2px;
}

// ─── Transitions ─────────────────────────────────────────────────────────────
.lt-slide-enter-active,
.lt-slide-leave-active {
  transition:
    opacity var(--duration-200) var(--ease-out),
    transform var(--duration-200) var(--ease-out),
    max-height var(--duration-300) var(--ease-out);
  overflow: hidden;
  max-height: 60px;
}

.lt-slide-enter-from,
.lt-slide-leave-to {
  opacity: 0;
  transform: translateY(-6px);
  max-height: 0;
}

.lt-fade-enter-active,
.lt-fade-leave-active {
  transition: opacity var(--duration-200) var(--ease-out);
}

.lt-fade-enter-from,
.lt-fade-leave-to {
  opacity: 0;
}

// ─── Keyframes ────────────────────────────────────────────────────────────────
@keyframes pulse-ring {
  0%,
  100% {
    opacity: 0.5;
    transform: scale(0.9);
  }
  50% {
    opacity: 1;
    transform: scale(1.1);
  }
}

@keyframes sync-pulse {
  0%,
  100% {
    opacity: 0.5;
    transform: scale(0.85);
  }
  50% {
    opacity: 1;
    transform: scale(1.15);
  }
}

@keyframes sync-blink {
  0%,
  100% {
    opacity: 1;
  }
  50% {
    opacity: 0.2;
  }
}

@keyframes spin-slow {
  from {
    transform: rotate(0deg);
  }
  to {
    transform: rotate(360deg);
  }
}
</style>
