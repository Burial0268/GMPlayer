<template>
  <div class="artistlists">
    <Transition mode="out-in">
      <n-grid
        x-gap="30"
        y-gap="34"
        cols="3 mb:4 s:5 l:6"
        responsive="screen"
        :collapsed="gridCollapsed"
        :collapsed-rows="gridCollapsedRows"
        v-if="listData[0]"
        key="data"
      >
        <n-gi
          class="item"
          v-for="item in listData"
          :key="item.id"
          role="link"
          tabindex="0"
          :data-navigation-identity="`artist:${item.id}`"
          @click="openArtist(item, $event)"
          @keydown.enter="openArtist(item, $event)"
          @contextmenu="openRightMenu($event, item)"
        >
          <div class="cover" data-navigation-cover>
            <n-avatar
              lazy
              :intersection-observer-options="avatarIntersectionOptions"
              class="coverImg"
              object-fit="cover"
              :img-props="{ 'data-navigation-shared-image': '' }"
              :src="item.cover.replace(/^http:/, 'https:') + '?param=200y200'"
              fallback-src="/images/pic/default.png"
            >
              <template #placeholder>
                <div class="cover-loading">
                  <n-spin size="small" />
                </div>
              </template>
              <template #fallback>
                <img data-navigation-shared-image src="/images/pic/default.png" alt="" />
              </template>
            </n-avatar>
            <n-avatar
              lazy
              :intersection-observer-options="avatarIntersectionOptions"
              round
              class="shadow"
              :src="item.cover.replace(/^http:/, 'https:') + '?param=200y200'"
              fallback-src="/images/pic/default.png"
            />
            <n-icon size="40" :component="PeopleSearchOne" />
          </div>
          <n-text class="name text-hidden" data-navigation-title>{{ item.name }}</n-text>
          <n-text class="size" :depth="3" v-if="item.size">
            {{
              $t("general.name.songSize", {
                size: item.size,
              })
            }}
          </n-text>
        </n-gi>
      </n-grid>
      <n-empty v-else-if="loading === false" key="empty" class="empty" />
      <n-grid
        v-else
        key="loading"
        class="loading"
        x-gap="20"
        y-gap="26"
        cols="3 mb:4 s:5 l:6"
        responsive="screen"
        :collapsed="gridCollapsed"
        :collapsed-rows="gridCollapsedRows"
      >
        <n-gi class="item" v-for="n in loadingNum" :key="n">
          <n-skeleton class="pic" :sharp="false" />
          <n-skeleton text style="width: 60%" />
        </n-gi>
      </n-grid>
    </Transition>
    <!-- 右键菜单 -->
    <n-dropdown
      style="--n-font-size: 14px; --n-border-radius: var(--radius-sm)"
      placement="bottom-start"
      trigger="manual"
      size="large"
      :x="rightMenuX"
      :y="rightMenuY"
      :options="rightMenuOptions"
      :show="rightMenuShow"
      :on-clickoutside="onClickoutside"
      @select="rightMenuShow = false"
    />
  </div>
</template>

<script setup>
import { NIcon } from "naive-ui";
import { PeopleSearchOne, LinkTwo, Like, Unlike } from "@icon-park/vue-next";
import { likeArtist } from "@/api/artist";
import { useLayerNavigation } from "@/utils/navigation";
import { userStore, settingStore } from "@/store";
import { useI18n } from "vue-i18n";

const { t } = useI18n();
const user = userStore();
const setting = settingStore();
const navigation = useLayerNavigation();
// NAvatar's observer path enables its error fallback when lazy loading is used.
const avatarIntersectionOptions = {};
const openArtist = (item, origin) =>
  navigation.openPage(`/artist/songs?id=${item.id}&page=1`, {
    origin,
    kind: "cover",
    identity: `artist:${item.id}`,
  });
const props = defineProps({
  // 列表数据
  listData: {
    type: Array,
    default: [],
  },
  // 折叠栅格
  gridCollapsed: {
    type: Boolean,
    default: false,
  },
  // 折叠后行数
  gridCollapsedRows: {
    type: Number,
    default: 1,
  },
  // 加载占位数量
  loadingNum: {
    type: Number,
    default: 6,
  },
  // 加载状态（null=旧行为，false=加载完成可显示空状态）
  loading: {
    type: Boolean,
    default: null,
  },
});

// 弹窗数据
const rightMenuX = ref(0);
const rightMenuY = ref(0);
const rightMenuShow = ref(false);
const rightMenuOptions = ref(null);

// 图标渲染
const renderIcon = (icon) => {
  return () => {
    return h(
      NIcon,
      { style: { transform: "translateX(2px)" } },
      {
        default: () => icon,
      },
    );
  };
};

// 打开右键菜单
const openRightMenu = (e, data) => {
  e.preventDefault();
  rightMenuShow.value = false;
  nextTick().then(() => {
    rightMenuOptions.value = [
      {
        key: "like",
        label: isLikeOrDislike(data.id)
          ? t("menu.collection", { name: t("general.name.artists") })
          : t("menu.cancelCollection", { name: t("general.name.artists") }),
        show: user.userLogin && user.getUserArtistLists.has ? true : false,
        icon: renderIcon(h(isLikeOrDislike(data.id) ? Like : Unlike)),
        props: {
          onClick: () => {
            toLikeArtist(data);
          },
        },
      },
      {
        key: "copy",
        label: t("menu.copy", {
          name: t("general.name.artists"),
          other: t("general.name.link"),
        }),
        icon: renderIcon(h(LinkTwo)),
        props: {
          onClick: () => {
            if (navigator.clipboard) {
              try {
                navigator.clipboard.writeText(`https://music.163.com/#/artist?id=${data.id}`);
                $message.success(t("general.message.copySuccess"));
              } catch (err) {
                console.error(t("general.message.copyFailure"), err);
                $message.error(t("general.message.copyFailure"));
              }
            } else {
              $message.error(t("general.message.notSupported"));
            }
          },
        },
      },
    ];
    rightMenuShow.value = true;
    rightMenuX.value = e.clientX;
    rightMenuY.value = e.clientY;
  });
};

// 点击右键菜单外部
const onClickoutside = () => {
  rightMenuShow.value = false;
};

// 收藏/取消收藏歌手
const toLikeArtist = (data) => {
  const type = isLikeOrDislike(data.id) ? 1 : 2;
  const isThereASpace = setting.language === "zh-CN" ? "" : " ";
  likeArtist(type, data.id).then((res) => {
    if (res.code === 200) {
      $message.success(
        `${data.name + isThereASpace}${
          type === 1
            ? t("menu.collection", { name: t("general.dialog.success") })
            : t("menu.cancelCollection", { name: t("general.dialog.success") })
        }`,
      );
      user.setUserArtistLists();
    } else {
      $message.error(
        `${data.name + isThereASpace}${
          type === 1
            ? t("menu.collection", { name: t("general.dialog.failed") })
            : t("menu.cancelCollection", { name: t("general.dialog.failed") })
        }`,
      );
    }
  });
};

// 判断收藏还是取消
const isLikeOrDislike = (id) => {
  return !user.getUserArtistIds.has(Number(id));
};

onMounted(() => {
  if (user.userLogin && !user.getUserArtistLists.has && !user.getUserArtistLists.isLoading)
    user.setUserArtistLists();
});
</script>

<style lang="scss" scoped>
.artistlists {
  padding-top: 20px;
  .v-enter-active,
  .v-leave-active {
    transition: opacity var(--duration-300) var(--ease-out);
  }

  .v-enter-from,
  .v-leave-to {
    opacity: 0;
  }
  .item {
    text-align: center;
    cursor: pointer;
    .cover {
      position: relative;
      display: flex;
      align-items: center;
      justify-content: center;
      .coverImg {
        width: 100%;
        height: 100%;
        overflow: visible;
        border-radius: 0;
        background: transparent;
        z-index: 1;
        :deep(img) {
          object-fit: cover;
          border-radius: 50%;
          box-shadow: 0 4px 16px 0 #00000020;
          transition:
            transform var(--duration-300) var(--ease-out),
            filter var(--duration-300) var(--ease-out),
            box-shadow var(--duration-300) var(--ease-out);
        }
        .cover-loading {
          position: relative;
          display: flex;
          align-items: center;
          justify-content: center;
          width: 100%;
          height: 0;
          padding-bottom: 100%;
          border-radius: 50%;
          background-color: #0001;
          .n-spin-body {
            position: absolute;
            top: 0;
            height: 100%;
            display: flex;
            align-items: center;
            justify-content: center;
          }
        }
      }
      .shadow {
        opacity: 0;
        position: absolute;
        top: 12px;
        height: 100%;
        width: 100%;
        filter: blur(16px) opacity(0.6);
        transform: scale(0.92, 0.96);
        z-index: 0;
        background-size: cover;
        aspect-ratio: 1/1;
        transition: opacity var(--duration-300) var(--ease-out);
      }
      .n-icon {
        opacity: 0;
        transform: scale(0.8);
        position: absolute;
        color: #fff;
        transition:
          opacity var(--duration-300) var(--ease-out),
          transform var(--duration-300) var(--ease-out);
        z-index: 1;
      }
      &:hover {
        .n-icon {
          opacity: 1;
          transform: scale(1);
        }
        :deep(.coverImg img) {
          box-shadow: 0 4px 16px 0 #00000040;
          filter: brightness(0.8);
          transform: scale(1.05);
        }
        .shadow {
          opacity: 1;
        }
      }
      &:active {
        :deep(.coverImg img) {
          transform: scale(1);
        }
      }
    }
    .name {
      margin-top: 14px;
      font-size: 16px;
      transition: color var(--duration-300) var(--ease-out);
      cursor: pointer;
      &:hover {
        color: var(--main-color);
      }
    }
  }
  .loading {
    .pic {
      padding-bottom: 100%;
      width: 100%;
      height: 0;
      border-radius: 50% !important;
      margin-bottom: 20px;
    }
  }
  .empty {
    margin: 40px 0;
  }
}
</style>
