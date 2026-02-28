<template>
  <n-modal
    v-model:show="showModal"
    preset="card"
    :title="getModalTitle"
    class="listen-together-modal"
    :style="{ width: isMobile ? '90%' : '420px' }"
    :bordered="false"
    transform-origin="center"
  >
    <div class="modal-content">
      <!-- 未加入房间时显示创建/加入选项 -->
      <template v-if="!listenTogether.isInRoom">
        <n-tabs type="segment" v-model:value="activeTab">
          <n-tab-pane name="create" :tab="t('other.listenTogether.createRoom')">
            <div class="tab-content">
              <n-alert v-if="!userStore.userLogin" type="warning" class="login-alert">
                {{ t("other.listenTogether.needLogin") }}
              </n-alert>
              <template v-else-if="!musicStore.getPlaySongData?.id">
                <n-empty :description="t('other.listenTogether.noSong')" />
              </template>
              <template v-else>
                <div class="current-song">
                  <img
                    :src="musicStore.getPlaySongData.album.picUrl + '?param=100y100'"
                    class="song-cover"
                    alt="cover"
                  />
                  <div class="song-info">
                    <div class="song-name">{{ musicStore.getPlaySongData.name }}</div>
                    <AllArtists :artistsData="musicStore.getPlaySongData.artist" />
                  </div>
                </div>
                <n-button
                  type="primary"
                  size="large"
                  block
                  :loading="isLoading"
                  @click="handleCreateRoom"
                >
                  {{ t("other.listenTogether.createRoom") }}
                </n-button>
              </template>
            </div>
          </n-tab-pane>

          <n-tab-pane name="join" :tab="t('other.listenTogether.joinRoom')">
            <div class="tab-content">
              <n-alert v-if="!userStore.userLogin" type="warning" class="login-alert">
                {{ t("other.listenTogether.needLogin") }}
              </n-alert>
              <template v-else>
                <n-input
                  v-model:value="joinRoomId"
                  :placeholder="t('other.listenTogether.roomIdPlaceholder')"
                  size="large"
                  clearable
                  maxlength="20"
                />
                <n-button
                  type="primary"
                  size="large"
                  block
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

      <!-- 已在房间中显示房间信息 -->
      <template v-else>
        <div class="room-info">
          <!-- 房间ID和分享 -->
          <div class="room-header">
            <div class="room-id-section">
              <span class="label">{{ t("other.listenTogether.roomId") }}</span>
              <div class="room-id-value">
                <code>{{ listenTogether.roomId }}</code>
                <n-button text size="small" @click="copyRoomId">
                  <template #icon>
                    <n-icon :component="CopyOutline" />
                  </template>
                </n-button>
              </div>
            </div>
            <n-button type="primary" size="small" @click="copyShareLink">
              <template #icon>
                <n-icon :component="ShareSocialOutline" />
              </template>
              {{ t("other.listenTogether.copyLink") }}
            </n-button>
          </div>

          <!-- 在线用户列表 -->
          <div class="users-section">
            <div class="section-title">
              {{ t("other.listenTogether.onlineUsers") }} ({{ listenTogether.onlineCount }})
            </div>
            <div class="users-list">
              <!-- 房主始终显示在列表首位 -->
              <div v-if="listenTogether.hostInfo" class="user-item host">
                <img :src="listenTogether.hostInfo.avatarUrl + '?param=50y50'" class="avatar" />
                <span class="nickname">{{ listenTogether.hostInfo.nickname }}</span>
                <n-tag size="small" type="primary">{{ t("other.listenTogether.host") }}</n-tag>
              </div>
              <div
                v-for="roomUser in listenTogether.users"
                :key="roomUser.userId"
                class="user-item"
              >
                <img :src="roomUser.avatarUrl + '?param=50y50'" class="avatar" />
                <!-- RoomUser.nickname 全小写 -->
                <span class="nickname">{{ roomUser.nickname }}</span>
                <n-tag
                  v-if="roomUser.userId === userStore.userData?.userId"
                  size="small"
                  type="success"
                >
                  {{ t("other.listenTogether.me") }}
                </n-tag>
              </div>
            </div>
          </div>

          <!-- 当前播放 -->
          <div v-if="listenTogether.isInRoom" class="now-playing">
            <div class="section-title">{{ t("general.name.play") }}</div>
            <div class="song-item">
              <img
                :src="musicStore.getPlaySongData?.album?.picUrl + '?param=60y60'"
                class="song-cover-small"
                alt="cover"
              />
              <div class="song-detail">
                <div class="song-name">{{ musicStore.getPlaySongData?.name || "-" }}</div>
                <AllArtists :artistsData="musicStore.getPlaySongData?.artist" />
              </div>
            </div>
          </div>

          <!-- 离开/关闭房间按钮 -->
          <n-button
            type="error"
            size="large"
            block
            ghost
            :loading="isLoading"
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
import { CopyOutline, ShareSocialOutline } from "@vicons/ionicons5";
import AllArtists from "@/components/DataList/AllArtists.vue";
import { useResponsiveLayout } from "@/composables/useResponsiveLayout";

const { t } = useI18n();
const musicStore = useMusicDataStore();
const userStore = useUserDataStore();
const listenTogether = useListenTogetherStore();
const { isMobile } = useResponsiveLayout();

const props = defineProps({
  show: {
    type: Boolean,
    default: false,
  },
});

const emit = defineEmits(["update:show"]);

const showModal = computed({
  get: () => props.show,
  set: (val) => emit("update:show", val),
});

const getModalTitle = computed(() => {
  if (listenTogether.isInRoom) {
    return listenTogether.isHost
      ? t("other.listenTogether.titleHost")
      : t("other.listenTogether.titleGuest");
  }
  return t("other.listenTogether.title");
});

const activeTab = ref("create");
const joinRoomId = ref("");
const isLoading = ref(false);

const handleCreateRoom = async () => {
  if (isLoading.value) return;
  isLoading.value = true;
  await listenTogether.createRoom();
  isLoading.value = false;
};

const handleJoinRoom = async () => {
  if (isLoading.value || !joinRoomId.value.trim()) return;
  isLoading.value = true;
  const success = await listenTogether.joinRoom(joinRoomId.value.trim());
  isLoading.value = false;
  if (success) {
    joinRoomId.value = "";
  }
};

const handleLeaveRoom = async () => {
  if (isLoading.value) return;
  isLoading.value = true;
  await listenTogether.leaveRoom();
  isLoading.value = false;
};

const copyRoomId = async () => {
  if (!listenTogether.roomId) return;
  try {
    await navigator.clipboard.writeText(listenTogether.roomId);
    $message.success(t("other.listenTogether.copySuccess"));
  } catch (err) {
    console.error("Copy failed:", err);
    $message.error(t("other.listenTogether.copyFailed"));
  }
};

const copyShareLink = async () => {
  const link = listenTogether.shareLink;
  if (!link) return;
  try {
    await navigator.clipboard.writeText(link);
    $message.success(t("other.listenTogether.copySuccess"));
  } catch (err) {
    console.error("Copy failed:", err);
    $message.error(t("other.listenTogether.copyFailed"));
  }
};
</script>

<style scoped lang="scss">
.listen-together-modal {
  :deep(.n-card__content) {
    padding: 16px;
  }

  .modal-content {
    min-height: 200px;
  }

  .tab-content {
    padding-top: 20px;
    display: flex;
    flex-direction: column;
    gap: 16px;

    .login-alert {
      margin-bottom: 8px;
    }

    .current-song {
      display: flex;
      align-items: center;
      gap: 12px;
      padding: 12px;
      background-color: var(--n-action-color);
      border-radius: 8px;

      .song-cover {
        width: 60px;
        height: 60px;
        border-radius: 6px;
        object-fit: cover;
      }

      .song-info {
        flex: 1;
        min-width: 0;

        .song-name {
          font-weight: 600;
          font-size: 14px;
          margin-bottom: 4px;
          overflow: hidden;
          text-overflow: ellipsis;
          white-space: nowrap;
        }

        :deep(.artists) {
          font-size: 12px;
          color: var(--n-text-color-3);
        }
      }
    }
  }

  .room-info {
    display: flex;
    flex-direction: column;
    gap: 20px;

    .room-header {
      display: flex;
      justify-content: space-between;
      align-items: flex-start;
      gap: 12px;
      padding-bottom: 16px;
      border-bottom: 1px solid var(--n-divider-color);

      .room-id-section {
        display: flex;
        flex-direction: column;
        gap: 4px;

        .label {
          font-size: 12px;
          color: var(--n-text-color-3);
        }

        .room-id-value {
          display: flex;
          align-items: center;
          gap: 8px;

          code {
            font-family: monospace;
            font-size: 18px;
            font-weight: 600;
            color: var(--main-color);
            background-color: var(--n-action-color);
            padding: 4px 8px;
            border-radius: 4px;
          }
        }
      }
    }

    .section-title {
      font-size: 13px;
      font-weight: 600;
      color: var(--n-text-color-2);
      margin-bottom: 10px;
      text-transform: uppercase;
      letter-spacing: 0.5px;
    }

    .users-section {
      .users-list {
        max-height: 150px;
        overflow-y: auto;

        .user-item {
          display: flex;
          align-items: center;
          gap: 10px;
          padding: 8px;
          border-radius: 6px;
          transition: background-color 0.2s;

          &:hover {
            background-color: var(--n-action-color);
          }

          &.host {
            background-color: var(--main-color-hover);
          }

          .avatar {
            width: 36px;
            height: 36px;
            border-radius: 50%;
            object-fit: cover;
          }

          .nickname {
            flex: 1;
            font-size: 14px;
            overflow: hidden;
            text-overflow: ellipsis;
            white-space: nowrap;
          }
        }
      }
    }

    .now-playing {
      padding: 12px;
      background-color: var(--n-action-color);
      border-radius: 8px;

      .song-item {
        display: flex;
        align-items: center;
        gap: 10px;

        .song-cover-small {
          width: 40px;
          height: 40px;
          border-radius: 4px;
          object-fit: cover;
        }

        .song-detail {
          flex: 1;
          min-width: 0;

          .song-name {
            font-size: 13px;
            font-weight: 500;
            overflow: hidden;
            text-overflow: ellipsis;
            white-space: nowrap;
          }

          :deep(.artists) {
            font-size: 11px;
            color: var(--n-text-color-3);
          }
        }
      }
    }
  }
}
</style>
