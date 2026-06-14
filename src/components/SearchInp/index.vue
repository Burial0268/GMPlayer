<template>
  <div class="searchInp">
    <n-input
      :class="site.searchInputActive ? 'input focus' : 'input'"
      :input-props="{ autoComplete: false }"
      :placeholder="$t('nav.search.placeholder')"
      ref="searchInpRef"
      round
      clearable
      v-model:value="inputValue"
      @focus="inputFocus"
      @keydown="inputkeydown($event)"
      @click.stop
    >
      <template #prefix>
        <n-icon size="16" :class="site.searchInputActive ? 'active' : ''" :component="Search" />
      </template>
    </n-input>
    <CollapseTransition easing="ease-in-out">
      <n-card
        class="list"
        v-show="
          site.searchInputActive && !inputValue && (music.getSearchHistory[0] || searchData.hot[0])
        "
        content-style="padding: 0"
      >
        <n-scrollbar>
          <div class="history-list" v-if="music.getSearchHistory[0] && setting.searchHistory">
            <div class="list-title">
              <n-icon size="16" :component="History" />
              <n-text>{{ $t("nav.search.history") }}</n-text>
            </div>
            <n-space>
              <n-tag
                v-for="item in music.getSearchHistory"
                :key="item"
                :bordered="false"
                round
                v-html="item"
                @click="toSearch(item, 0)"
              />
            </n-space>
            <div class="del" @click="delHistory">
              <n-icon size="16" :depth="3">
                <DeleteFour theme="filled" />
              </n-icon>
              <n-text :depth="3">{{ $t("nav.search.delHistory") }}</n-text>
            </div>
          </div>
          <div class="hot-list" v-if="searchData.hot[0]">
            <div class="list-title">
              <n-icon size="16">
                <Fire theme="filled" />
              </n-icon>
              <n-text>{{ $t("nav.search.hotList") }}</n-text>
            </div>
            <div
              class="hot-item"
              v-for="(item, index) in searchData.hot"
              :key="item"
              @click="toSearch(item.searchWord, 0)"
            >
              <div :class="index < 3 ? 'num hot' : 'num'">{{ index + 1 }}</div>
              <div class="title">
                <span class="name">
                  {{ item.searchWord }}
                  <!-- <img :src="item.iconUrl" alt="icon" /> -->
                  <n-tag v-if="item.iconUrl" class="tag" round :bordered="false" size="small">
                    {{ item.iconType == 1 ? "HOT" : "UP" }}
                  </n-tag>
                </span>
                <n-text class="tip" depth="3" v-html="item.content" />
              </div>
            </div>
          </div>
        </n-scrollbar>
      </n-card>
    </CollapseTransition>
    <CollapseTransition easing="ease-in-out">
      <n-card
        class="list"
        v-show="site.searchInputActive && inputValue && searchData.suggest"
        content-style="padding: 0"
      >
        <n-scrollbar>
          <div class="suggest-tip" v-if="Object.keys(searchData.suggest).length === 0">
            <n-icon size="16" :component="Find" />
            <span>{{ $t("nav.search.noSuggestions") }}</span>
          </div>
          <div class="suggest-all" v-else>
            <div class="loading" v-show="!searchData.suggest.order">
              <n-icon size="16" :component="Find" />
              <span>{{ $t("nav.search.searchTip") }}</span>
            </div>
            <div class="suggest-item" v-if="searchData.suggest.songs">
              <div class="type">
                <n-icon size="18">
                  <MusicOne theme="filled" />
                </n-icon>
                <span class="name">{{ $t("nav.search.songs") }}</span>
              </div>
              <span
                class="names"
                v-for="songs in searchData.suggest.songs"
                :key="songs"
                @click="toSearch(songs.id, 1)"
              >
                {{ songs.name }} - {{ songs.artists[0].name }}</span
              >
            </div>
            <div class="suggest-item" v-if="searchData.suggest.artists">
              <div class="type">
                <n-icon size="18">
                  <Voice theme="filled" />
                </n-icon>
                <span class="name">{{ $t("nav.search.artists") }}</span>
              </div>
              <span
                class="names"
                v-for="artists in searchData.suggest.artists"
                :key="artists"
                @click="toSearch(artists.id, 100)"
                v-html="artists.name"
              />
            </div>
            <div class="suggest-item" v-if="searchData.suggest.albums">
              <div class="type">
                <n-icon size="18">
                  <RecordDisc theme="filled" />
                </n-icon>
                <span class="name">{{ $t("nav.search.albums") }}</span>
              </div>
              <span
                class="names"
                v-for="albums in searchData.suggest.albums"
                :key="albums"
                @click="toSearch(albums.id, 10)"
              >
                {{ albums.name }} - {{ albums.artist.name }}
              </span>
            </div>
            <div class="suggest-item" v-if="searchData.suggest.playlists">
              <div class="type">
                <n-icon size="18">
                  <Record theme="filled" />
                </n-icon>
                <span class="name">{{ $t("nav.search.playlists") }}</span>
              </div>
              <span
                class="names"
                v-for="playlists in searchData.suggest.playlists"
                :key="playlists"
                @click="toSearch(playlists.id, 1000)"
              >
                {{ playlists.name }}
              </span>
            </div>
          </div>
        </n-scrollbar>
      </n-card>
    </CollapseTransition>
  </div>
</template>

<script setup>
import { getSearchHot, getSearchSuggest } from "@/api/search";
import {
  Search,
  MusicOne,
  Voice,
  RecordDisc,
  Record,
  Find,
  Fire,
  History,
  DeleteFour,
} from "@icon-park/vue-next";
import { useI18n } from "vue-i18n";
import { useRouter } from "vue-router";
import { musicStore, settingStore, siteStore } from "@/store";
import CollapseTransition from "@ivanv/vue-collapse-transition/src/CollapseTransition.vue";
import debounce from "@/utils/debounce";

const { t } = useI18n();
const router = useRouter();
const music = musicStore();
const setting = settingStore();
const site = siteStore();

// 输入框内容
const inputValue = ref(null);
const searchInpRef = ref(null);

// 输入框激活事件
const inputFocus = () => {
  searchInpRef.value?.focus();
  site.searchInputActive = true;
  music.showPlayList = false;
  getSearchHotData();
};

// 搜索相关数据
const searchData = reactive({
  hot: [], // 热搜
  suggest: {}, // 搜索建议
});

// 获取搜索相关数据
const getSearchHotData = () => {
  getSearchHot().then((res) => {
    searchData.hot = res.data;
  });
};
const getSearchSuggestData = (keywords) => {
  searchData.suggest = [];
  getSearchSuggest(keywords).then((res) => {
    console.log(res);
    searchData.suggest = res.result;
  });
};

// 点击搜索结果
const toSearch = (val, type) => {
  // 非直接搜索时关闭搜索面板
  if (type !== 0) {
    site.searchInputActive = false;
  }
  switch (type) {
    case 0:
      // 直接搜索
      inputValue.value = val;
      // 写入搜索历史
      music.setSearchHistory(inputValue.value.trim());
      router.push({
        path: "/search/songs",
        query: {
          keywords: val,
          page: 1,
        },
      });
      break;
    case 1:
      // 歌曲页
      router.push(`/song?id=${val}`);
      break;
    case 10:
      // 专辑页
      router.push(`/album?id=${val}`);
      break;
    case 100:
      // 歌手页
      router.push(`/artist?id=${val}`);
      break;
    case 1000:
      // 歌单页
      router.push({
        path: "/playlist",
        query: { id: val, page: 1 },
      });
      break;
    default:
      break;
  }
};

// 回车搜索
const inputkeydown = (e) => {
  if (e.key === "Enter" && inputValue.value !== null) {
    console.log("执行搜索" + inputValue.value.trim());
    searchInpRef.value?.blur();
    site.searchInputActive = false;
    // 写入搜索历史
    music.setSearchHistory(inputValue.value.trim());
    router.push({
      path: "/search/songs",
      query: {
        keywords: inputValue.value.trim(),
      },
    });
  }
};

const closeSearchPanel = () => {
  searchInpRef.value?.blur();
  site.searchInputActive = false;
};

// 删除搜索历史
const delHistory = () => {
  $dialog.warning({
    class: "s-dialog",
    title: t("general.dialog.delete"),
    content: t("nav.search.tip"),
    positiveText: t("general.dialog.delete"),
    negativeText: t("general.dialog.cancel"),
    onPositiveClick: () => {
      music.setSearchHistory(null, true);
      $message.success(t("general.message.deleteSuccess"));
    },
  });
};

onMounted(() => {
  // 获取热搜
  getSearchHotData();
  // 搜索框失焦
  document.addEventListener("click", closeSearchPanel);
});

onUnmounted(() => {
  document.removeEventListener("click", closeSearchPanel);
});

// 监听输入框内容
watch(
  () => inputValue.value,
  (value) => {
    if (value.trim()) {
      debounce(() => {
        console.log(value.trim());
        getSearchSuggestData(value.trim());
      }, 500);
    }
  },
);

// 监听播放列表显隐
watch(
  () => music.showPlayList,
  (val) => {
    if (val) {
      searchInpRef.value?.blur();
      site.searchInputActive = false;
    }
  },
);
</script>

<style lang="scss" scoped>
.searchInp {
  position: relative;
  width: clamp(180px, 22vw, 260px);
  display: flex;
  justify-content: flex-start;
  pointer-events: none;

  @media (max-width: 450px) {
    width: auto;
  }

  .input {
    --n-color: var(--layout-bg, #fff);
    --n-color-focus: var(--layout-bg, #fff);
    --n-color-hover: var(--layout-bg, #fff);
    --n-border: 1px solid var(--acrylic-border, rgba(0, 0, 0, 0.08));
    --n-border-hover: 1px solid var(--main-color);
    --n-border-focus: 1px solid var(--main-color);
    --n-box-shadow-focus: 0 0 0 2px var(--main-second-color);
    pointer-events: auto;
    width: 100%;
    height: 32px;
    overflow: hidden;
    background-color: var(--layout-bg, #fff);
    -webkit-backdrop-filter: blur(18px) saturate(160%);
    backdrop-filter: blur(18px) saturate(160%);
    box-shadow:
      0 8px 24px rgb(0 0 0 / 8%),
      inset 0 0 0 1px var(--acrylic-border, rgba(255, 255, 255, 0.16));
    transition:
      width 0.3s,
      background-color 0.2s,
      box-shadow 0.2s;

    @media (max-width: 450px) {
      width: 36px;
    }

    &.focus {
      width: 100%;
      background-color: var(--layout-bg, #fff);

      :deep(input) {
        color: var(--main-color);
      }

      @media (max-width: 450px) {
        width: min(54vw, 220px);
      }

      @media (max-width: 380px) {
        width: 50vw;
      }

      @media (max-width: 320px) {
        width: 48vw;
      }
    }

    :deep(.n-input-wrapper) {
      padding-inline: 10px;
    }

    :deep(.n-input__input-el) {
      height: 32px;
      font-size: 13px;
    }

    :deep(.n-input__prefix) {
      margin-right: 4px;
    }

    :deep(.n-input__prefix) {
      .n-icon {
        transition: color 0.3s;
        &.active {
          color: var(--main-color);
        }
      }
    }
  }
  .list {
    --n-color: var(--layout-bg, #fff);
    --n-border-color: var(--acrylic-border, rgba(0, 0, 0, 0.08));
    position: absolute;
    top: calc(var(--app-safe-area-top, 0px) + 38px);
    left: 0;
    border-radius: 10px;
    width: 280px;
    z-index: 3;
    pointer-events: auto;
    overflow: hidden;
    background-color: var(--layout-bg, #fff);
    -webkit-backdrop-filter: blur(24px) saturate(160%);
    backdrop-filter: blur(24px) saturate(160%);
    box-shadow:
      0 18px 46px rgb(0 0 0 / 14%),
      inset 0 0 0 1px var(--acrylic-border, rgba(255, 255, 255, 0.14));

    @media (max-width: 450px) {
      position: fixed;
      width: auto;
      top: calc(var(--app-safe-area-top, 0px) + 56px);
      right: 12px;
      left: 12px;
      border-radius: 14px;
      z-index: 2006;
    }

    :deep(.n-card__content) {
      background-color: var(--layout-bg, #fff);
    }

    :deep(.n-scrollbar) {
      max-height: 68vh;
      @media (max-width: 450px) {
        max-height: min(58vh, calc(100vh - var(--app-safe-area-top, 0px) - 148px));
        box-sizing: border-box;
      }
      .n-scrollbar-rail {
        width: 4px;
      }
      .n-scrollbar-container {
        @media (max-width: 450px) {
          padding-top: 8px;
        }
        .n-scrollbar-content {
          padding: 10px;
          .list-title {
            color: var(--main-color);
            display: flex;
            align-items: center;
            margin-bottom: 6px;
            .n-text {
              margin-left: 4px;
              font-size: 13px;
              color: var(--main-color);
              line-height: 0;
            }
          }
          .history-list {
            margin-bottom: 14px;
            .n-space {
              margin: 10px 0;
              .n-tag {
                font-size: 12px;
                cursor: pointer;
                transition: all 0.3s;
                &:hover {
                  background-color: var(--main-second-color);
                  color: var(--main-color);
                }
                &:active {
                  transform: scale(0.95);
                }
              }
            }
            .del {
              display: flex;
              align-items: center;
              justify-content: center;
              font-size: 12px;
              cursor: pointer;
              .n-icon {
                margin-right: 4px;
              }
            }
          }
          .hot-list {
            margin-top: 4px;
            .hot-item {
              display: flex;
              flex-direction: row;
              align-items: center;
              margin-bottom: 6px;
              cursor: pointer;
              border-radius: 8px;
              padding: 5px;
              transition: all 0.3s;

              &:nth-last-of-type(1) {
                margin-bottom: 0;
              }

              &:hover {
                background-color: var(--n-border-color);
              }
              .num {
                width: 26px;
                height: 26px;
                min-width: 26px;
                text-align: center;
                line-height: 26px;
                font-size: 14px;
                font-weight: bold;
                margin-right: 6px;
                &.hot {
                  color: var(--main-color);
                }
              }
              .title {
                display: flex;
                flex-direction: column;
                .name {
                  font-size: 14px;
                  display: flex;
                  flex-direction: row;
                  align-items: center;
                  img {
                    height: 16px;
                    width: auto;
                    margin-left: 6px;
                    margin-bottom: 2px;
                  }
                  .tag {
                    transform: scale(0.9);
                    margin-left: 6px;
                    height: 18px;
                    color: var(--main-color);
                    background-color: var(--main-second-color);
                    border-color: var(--main-color);
                  }
                }
                .tip {
                  font-size: 12px;
                }
              }
            }
          }
          .suggest-tip {
            display: flex;
            flex-direction: row;
            justify-content: center;
            align-items: center;
            .n-icon {
              margin-right: 6px;
            }
          }
          .suggest-all {
            .loading {
              display: flex;
              flex-direction: row;
              justify-content: center;
              align-items: center;
              .n-icon {
                margin-right: 6px;
              }
            }
            .suggest-item {
              margin-bottom: 10px;
              &:nth-last-of-type(1) {
                margin-bottom: 0;
              }
              .type {
                color: var(--main-color);
                display: flex;
                flex-direction: row;
                align-items: center;
                margin-bottom: 4px;
                .n-icon {
                  margin-bottom: 2px;
                }
                .name {
                  font-size: 13px;
                  margin-left: 4px;
                }
              }
              .names {
                display: block;
                padding: 10px 12px 10px 16px;
                font-size: 13px;
                cursor: pointer;
                transition: all 0.3s;
                border-radius: 8px;
                &:hover {
                  background-color: var(--n-border-color);
                }
              }
            }
          }
        }
      }
    }
  }
}
</style>
