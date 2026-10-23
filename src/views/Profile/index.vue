<template>
  <div class="profile" v-if="uid">
    <div class="head">
      <n-avatar round class="avatar" :src="avatarUrl" fallback-src="/images/ico/user-filling.svg" />
      <div class="meta">
        <span class="kind">{{ $t("profile.title") }}</span>
        <div class="name-row">
          <n-text class="name">{{ profile.nickname || "-" }}</n-text>
          <n-tag v-if="profile.vipType > 0" class="badge vip" size="small" round :bordered="false">
            {{ $t("profile.vip") }}
          </n-tag>
          <n-tag
            v-if="detail.level != null"
            class="badge level"
            size="small"
            round
            :bordered="false"
          >
            {{ $t("profile.level", { level: detail.level }) }}
          </n-tag>
        </div>
        <p class="sign">{{ profile.signature || $t("profile.signature") }}</p>
        <div class="stats">
          <div class="stat">
            <strong>{{ formatNumber(profile.follows || 0) }}</strong>
            <span>{{ $t("profile.follows") }}</span>
          </div>
          <div class="stat">
            <strong>{{ formatNumber(profile.followeds || 0) }}</strong>
            <span>{{ $t("profile.followeds") }}</span>
          </div>
          <div class="stat">
            <strong>{{ formatNumber(profile.playlistCount || 0) }}</strong>
            <span>{{ $t("profile.playlistCount") }}</span>
          </div>
          <div class="stat" v-if="detail.listenSongs">
            <strong>{{ formatNumber(detail.listenSongs) }}</strong>
            <span>{{ $t("profile.listenSongs") }}</span>
          </div>
        </div>
        <div class="control" v-if="showFollow">
          <n-button
            strong
            secondary
            round
            :type="followed ? 'default' : 'primary'"
            :loading="followLoading"
            @click="toggleFollow"
          >
            <template #icon>
              <n-icon :component="followed ? PersonRemoveAlt1Round : PersonAddAlt1Round" />
            </template>
            {{ followed ? $t("profile.unfollow") : $t("profile.follow") }}
          </n-button>
        </div>
      </div>
    </div>

    <n-tabs class="main-tab" type="line" v-model:value="tabValue">
      <n-tab name="created">{{ $t("profile.created") }}</n-tab>
      <n-tab name="collected">{{ $t("profile.collected") }}</n-tab>
    </n-tabs>

    <main class="content">
      <Transition name="fade" mode="out-in">
        <CoverLists
          v-if="tabValue === 'created'"
          key="created"
          :listData="ownLists"
          :loading="loading"
        />
        <CoverLists v-else key="collected" :listData="likeLists" :loading="loading" />
      </Transition>
      <div class="load-more" v-if="!loading && hasMore">
        <n-button strong secondary round :loading="loadingMore" @click="loadMorePlaylists">
          {{ $t("general.name.loadMore") }}
        </n-button>
      </div>
    </main>
  </div>

  <div class="title" v-else>
    <span class="key">{{ $t("general.name.noKeywords") }}</span>
    <br />
    <n-button strong secondary @click="router.go(-1)" style="margin-top: 20px">
      {{ $t("general.name.goBack") }}
    </n-button>
  </div>
</template>

<script setup lang="ts">
import { useRouter } from "vue-router";
import { useI18n } from "vue-i18n";
import { PersonAddAlt1Round, PersonRemoveAlt1Round } from "@vicons/material";
import { userStore } from "@/store";
import { getUserDetail, getUserPlaylist, followUser } from "@/api/user";
import { formatNumber } from "@/utils/timeTools";
import CoverLists from "@/components/DataList/CoverLists.vue";

const { t } = useI18n();
const router = useRouter();
const user = userStore();

const uid = ref(router.currentRoute.value.query.id);
const detail = ref<any>({});
const profile = ref<any>({});
const ownLists = ref<any[]>([]);
const likeLists = ref<any[]>([]);
const loading = ref(true);
const loadingMore = ref(false);
const hasMore = ref(false);
const followed = ref(false);
const followLoading = ref(false);
const tabValue = ref("created");

// 歌单分批拉取。网易返回顺序是「自建在前、收藏在后」，因此按页追加不会打乱
// 下面 creator.userId 的归类；一次性 limit=1001 会让 CoverLists 直接铺开上千格。
const PLAYLIST_PAGE_SIZE = 100;
let requestSerial = 0;

// 头像地址
const avatarUrl = computed(() =>
  profile.value?.avatarUrl
    ? profile.value.avatarUrl.replace(/^http:/, "https:") + "?param=200y200"
    : "/images/ico/user-filling.svg",
);

// 是否显示关注按钮（登录且非本人）
const showFollow = computed(
  () => user.userLogin && Number(uid.value) !== Number(user.getUserData?.userId),
);

// 归类单页歌单：创建者是本人的进「自建」，否则进「收藏」
const collectPlaylists = (playlist: any[], id: number) => {
  playlist.forEach((v: any) => {
    const item = {
      id: v.id,
      cover: v.coverImgUrl,
      name: v.name,
      artist: v.creator,
      playCount: formatNumber(v.playCount),
      trackCount: v.trackCount,
    };
    if (v.creator?.userId === id) {
      ownLists.value.push(item);
    } else {
      likeLists.value.push(item);
    }
  });
};

// 加载用户信息与歌单
const loadProfile = async (id: string | string[] | null) => {
  if (!id) return;
  const requestId = ++requestSerial;
  const numericId = Number(id);
  loading.value = true;
  loadingMore.value = false;
  hasMore.value = false;
  ownLists.value = [];
  likeLists.value = [];
  try {
    // 两个请求互不依赖，并行发出，少一个往返
    const [detailRes, listRes] = await Promise.all([
      getUserDetail(numericId),
      getUserPlaylist(numericId, PLAYLIST_PAGE_SIZE, 0),
    ]);
    if (requestId !== requestSerial) return;

    detail.value = detailRes || {};
    profile.value = detailRes?.profile || {};
    followed.value = Boolean(profile.value?.followed);
    $setSiteTitle(profile.value?.nickname || t("profile.title"));

    if (listRes?.playlist) collectPlaylists(listRes.playlist, numericId);
    hasMore.value = Boolean(listRes?.more ?? listRes?.playlist?.length === PLAYLIST_PAGE_SIZE);

    loading.value = false;
    if (typeof $scrollToTop !== "undefined") $scrollToTop();
  } catch (err) {
    if (requestId !== requestSerial) return;
    loading.value = false;
    console.error(t("general.message.acquisitionFailed"), err);
    $message.error(t("general.message.acquisitionFailed"));
    router.go(-1);
  }
};

// 加载下一页歌单
const loadMorePlaylists = async () => {
  if (loadingMore.value || !hasMore.value) return;
  const requestId = requestSerial;
  const numericId = Number(uid.value);
  const offset = ownLists.value.length + likeLists.value.length;
  loadingMore.value = true;
  try {
    const res = await getUserPlaylist(numericId, PLAYLIST_PAGE_SIZE, offset);
    if (requestId !== requestSerial) return;
    if (res?.playlist) collectPlaylists(res.playlist, numericId);
    hasMore.value = Boolean(res?.more ?? res?.playlist?.length === PLAYLIST_PAGE_SIZE);
  } catch (err) {
    console.error(t("general.message.acquisitionFailed"), err);
  } finally {
    if (requestId === requestSerial) loadingMore.value = false;
  }
};

// 关注 / 取消关注
const toggleFollow = async () => {
  if (followLoading.value) return;
  followLoading.value = true;
  const type = followed.value ? 0 : 1;
  try {
    const res = await followUser(Number(uid.value), type);
    if (res.code === 200) {
      followed.value = !followed.value;
      $message.success(type === 1 ? t("profile.followSuccess") : t("profile.unfollowSuccess"));
    } else {
      $message.error(res.message || t("general.message.operationFailed"));
    }
  } catch (err) {
    console.error(t("general.message.operationFailed"), err);
    $message.error(t("general.message.operationFailed"));
  } finally {
    followLoading.value = false;
  }
};

onMounted(() => {
  loadProfile(uid.value);
});

// 监听路由参数变化（在不同主页间跳转）
watch(
  () => [router.currentRoute.value.name, router.currentRoute.value.query.id],
  ([name, id]) => {
    if (name !== "profile" || id === uid.value) return;
    uid.value = id as string;
    tabValue.value = "created";
    loadProfile(uid.value);
  },
);
</script>

<style lang="scss" scoped>
.profile {
  display: flex;
  flex-direction: column;
  gap: 18px;
  padding: 10px clamp(16px, 3vw, 36px) 36px;

  .head {
    display: grid;
    grid-template-columns: minmax(120px, 176px) minmax(0, 1fr);
    align-items: center;
    gap: clamp(22px, 4vw, 38px);
    padding: 18px 2px;

    .avatar {
      width: 100%;
      height: auto;
      aspect-ratio: 1 / 1;
      box-shadow: 0 12px 28px -8px rgb(0 0 0 / 32%);
    }

    .meta {
      min-width: 0;
      display: flex;
      flex-direction: column;

      .kind {
        margin-bottom: 7px;
        font-size: 11px;
        font-weight: 700;
        line-height: 1;
        text-transform: uppercase;
        color: var(--n-text-color-3);
      }

      .name-row {
        display: flex;
        align-items: center;
        flex-wrap: wrap;
        gap: 10px;

        .name {
          font-size: clamp(28px, 4.4vw, 48px);
          font-weight: 700;
          line-height: 1.08;
          overflow-wrap: anywhere;
        }

        .badge {
          font-weight: 700;
          &.vip {
            color: #fff;
            background-color: #ec4141;
          }
          &.level {
            color: var(--n-text-color-2);
            background-color: color-mix(in srgb, var(--n-text-color) 10%, transparent);
          }
        }
      }

      .sign {
        margin: 12px 0 0;
        max-width: 720px;
        font-size: 14px;
        line-height: 1.6;
        color: var(--n-text-color-3);
        overflow-wrap: anywhere;
      }

      .stats {
        display: flex;
        flex-wrap: wrap;
        gap: 22px;
        margin-top: 16px;

        .stat {
          display: flex;
          flex-direction: column;
          strong {
            font-size: 18px;
            font-weight: 700;
            font-variant-numeric: tabular-nums;
          }
          span {
            margin-top: 2px;
            font-size: 12px;
            color: var(--n-text-color-3);
          }
        }
      }

      .control {
        margin-top: 18px;
      }
    }
  }

  .main-tab {
    margin-top: 0;
  }

  .content {
    position: relative;
    overflow: hidden;

    .fade-enter-active,
    .fade-leave-active {
      transition: opacity var(--duration-200) var(--ease-in-out);
    }
    .fade-enter-from,
    .fade-leave-to {
      opacity: 0;
    }

    .load-more {
      display: flex;
      justify-content: center;
      margin-top: 28px;
    }
  }

  @media (max-width: 640px) {
    gap: 14px;
    padding: 8px 14px 28px;

    .head {
      grid-template-columns: 96px minmax(0, 1fr);
      gap: 16px;
      padding: 12px 0;

      .meta {
        .name-row .name {
          font-size: clamp(24px, 8vw, 34px);
        }
        .stats {
          gap: 16px;
          .stat strong {
            font-size: 16px;
          }
        }
      }
    }
  }
}

.title {
  margin-top: 30px;
  margin-bottom: 20px;
  font-size: 24px;

  .key {
    margin-right: 8px;
    font-size: 40px;
    font-weight: bold;
  }
}
</style>
