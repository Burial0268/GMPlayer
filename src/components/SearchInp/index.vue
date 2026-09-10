<template>
  <div ref="searchAnchorRef" class="search-anchor">
    <div
      v-if="isMobile"
      class="searchInp search-trigger"
      role="button"
      tabindex="0"
      :aria-label="$t('navigation.search')"
      @click="openSearchPanel"
      @keydown.enter="openSearchPanel"
      @keydown.space.prevent="openSearchPanel"
    >
      <n-input
        class="input"
        round
        readonly
        :input-props="{ tabindex: -1 }"
        :placeholder="$t('nav.search.placeholder')"
        aria-hidden="true"
      >
        <template #prefix><n-icon size="16" :component="Search" /></template>
      </n-input>
    </div>
    <Teleport to="body" :disabled="!isMobile">
      <div
        ref="searchRootRef"
        v-show="!isMobile || rendered"
        :class="['searchInp', { active, 'mobile-search-layer': isMobile }]"
        :data-navigation-layer="active ? 'search' : undefined"
        :role="active ? 'dialog' : undefined"
        :aria-modal="active || undefined"
        :aria-label="$t('navigation.search')"
        :inert="isMobile && !active"
      >
        <div class="search-toolbar">
          <n-input
            :class="active ? 'input focus' : 'input'"
            :input-props="{
              autocomplete: 'off',
              enterkeyhint: 'search',
              'aria-label': $t('navigation.search'),
            }"
            :placeholder="$t('nav.search.placeholder')"
            ref="searchInpRef"
            round
            clearable
            v-model:value="inputValue"
            @focus="openSearchPanel"
            @keydown="inputkeydown($event)"
            @pointerdown.stop="openSearchPanel"
            @touchstart.stop="openSearchPanel"
            @click.stop="openSearchPanel"
          >
            <template #prefix>
              <n-icon
                size="16"
                :class="active ? 'active' : ''"
                :component="Search"
                @pointerdown.stop
                @touchstart.stop
                @click.stop="toggleSearchPanel"
              />
            </template>
          </n-input>
          <button v-if="active" type="button" class="search-cancel" @click="closeSearchPanelState">
            {{ $t("navigation.cancel") }}
          </button>
        </div>
        <div class="search-results-slot">
          <n-card
            class="list"
            v-show="active && !inputValue"
            content-style="padding: 0"
            @pointerdown.stop
            @touchstart.stop
            @click.stop
          >
            <n-scrollbar>
              <div class="suggest-tip" v-if="!music.getSearchHistory[0] && !searchData.hot[0]">
                <n-icon size="16" :component="Find" />
                <span>{{ $t("nav.search.searchTip") }}</span>
              </div>
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
        </div>
        <div class="search-results-slot">
          <n-card
            class="list"
            v-show="active && inputValue && searchData.suggest"
            content-style="padding: 0"
            @pointerdown.stop
            @touchstart.stop
            @click.stop
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
                    role="link"
                    tabindex="0"
                    @keydown.enter="toSearch(songs.id, 1, $event)"
                    @click="toSearch(songs.id, 1, $event)"
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
                    role="link"
                    tabindex="0"
                    @keydown.enter="toSearch(artists.id, 100, $event)"
                    @click="toSearch(artists.id, 100, $event)"
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
                    role="link"
                    tabindex="0"
                    @keydown.enter="toSearch(albums.id, 10, $event)"
                    @click="toSearch(albums.id, 10, $event)"
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
                    role="link"
                    tabindex="0"
                    @keydown.enter="toSearch(playlists.id, 1000, $event)"
                    @click="toSearch(playlists.id, 1000, $event)"
                  >
                    {{ playlists.name }}
                  </span>
                </div>
              </div>
            </n-scrollbar>
          </n-card>
        </div>
      </div>
    </Teleport>
  </div>
</template>

<script setup>
import { computed, nextTick, onBeforeUnmount, onMounted, reactive, ref, watch } from "vue";
import { animateMini } from "motion-v";
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
import { musicStore, settingStore } from "@/store";
import { useLayerNavigation } from "@/utils/navigation";
import { useResponsiveLayout } from "@/composables/useResponsiveLayout";
import { layerMotion, motionDuration } from "@/utils/navigation/motion";
import { isRestoringSourceFocus } from "@/utils/navigation/sources";
import { useMotionInterruption } from "@/composables/useMotionInterruption";

const props = defineProps({ location: { type: String, default: "sidebar" } });
const { t } = useI18n();
const navigation = useLayerNavigation();
const music = musicStore();
const setting = settingStore();
const { isMobile } = useResponsiveLayout();
const ownsSearch = computed(() => props.location === (isMobile.value ? "nav" : "sidebar"));
const active = computed(() => ownsSearch.value && navigation.searchVisible.value);
const presented = computed(
  () =>
    ownsSearch.value && navigation.presentedOverlays.value.some((layer) => layer.kind === "search"),
);
const draft = ref("");
const inputValue = computed({
  get: () => navigation.searchLayer.value?.search?.query ?? draft.value,
  set: (value) => {
    draft.value = value ?? "";
    navigation.updateSearchQuery(draft.value);
  },
});
const searchInpRef = ref(null);
const searchRootRef = ref(null);
const searchAnchorRef = ref(null);
const rendered = ref(false);
const searchHotLoading = ref(false);
const searchData = reactive({ hot: [], suggest: {} });
let suggestTimer;
let suggestGeneration = 0;
let animation;
let animationGeneration = 0;
let suspended = false;

const getSearchHotData = async () => {
  if (searchHotLoading.value || searchData.hot.length) return;
  searchHotLoading.value = true;
  try {
    searchData.hot = (await getSearchHot()).data ?? [];
  } catch (error) {
    console.warn("[search] hot searches failed", error);
  } finally {
    searchHotLoading.value = false;
  }
};
const getSearchSuggestData = async (query, generation) => {
  try {
    const response = await getSearchSuggest(query);
    if (generation === suggestGeneration && inputValue.value.trim() === query)
      searchData.suggest = response.result ?? {};
  } catch (error) {
    if (generation === suggestGeneration) searchData.suggest = {};
    console.warn("[search] suggestions failed", error);
  }
};
const openSearchPanel = (event) => {
  if (active.value || (event?.type === "focus" && isRestoringSourceFocus())) return;
  void navigation.openSearch(
    isMobile.value
      ? searchAnchorRef.value?.querySelector(".search-trigger")
      : (event?.currentTarget ?? searchRootRef.value),
  );
};
const closeSearchPanelState = () => {
  searchInpRef.value?.blur();
  navigation.closeTop("search");
};
const toggleSearchPanel = (event) =>
  active.value ? closeSearchPanelState() : openSearchPanel(event);
const toSearch = (value, type, event) => {
  searchInpRef.value?.blur();
  if (type === 0) {
    const query = String(value ?? "").trim();
    if (!query) return;
    inputValue.value = query;
    music.setSearchHistory(query);
    void navigation.replacePage({ path: "/search/songs", query: { keywords: query, page: 1 } });
    return;
  }
  const path = { 1: "/song", 10: "/album", 100: "/artist", 1000: "/playlist" }[type];
  if (path)
    void navigation.openPage(
      { path, query: { id: value, ...(type === 1000 ? { page: 1 } : {}) } },
      { origin: event },
    );
};
const inputkeydown = (event) => {
  if (event.key === "Enter" && !event.isComposing) toSearch(inputValue.value, 0);
};
const closeSearchPanel = (event) => {
  if (!active.value || isMobile.value || searchRootRef.value?.contains(event.target)) return;
  closeSearchPanelState();
};
const delHistory = () =>
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

const finishMotion = () => {
  animationGeneration++;
  animation?.cancel();
  animation = undefined;
  searchRootRef.value?.style.removeProperty("clip-path");
  rendered.value = presented.value;
};
watch(
  presented,
  async (show) => {
    const generation = ++animationGeneration;
    animation?.cancel();
    animation = undefined;
    const resume = suspended && show;
    suspended = !show && navigation.hasLayer("search");
    if (show) rendered.value = true;
    if (!isMobile.value || resume || suspended) {
      rendered.value = show;
      searchRootRef.value?.style.removeProperty("clip-path");
      return;
    }
    await nextTick();
    if (generation !== animationGeneration || !searchRootRef.value) return;
    const anchor = searchAnchorRef.value?.getBoundingClientRect();
    const collapsed = anchor
      ? `inset(${anchor.top}px calc(100% - ${anchor.right}px) calc(100% - ${anchor.bottom}px) ${anchor.left}px round 22px)`
      : "inset(0px 0px 100% 0px round 22px)";
    const full = "inset(0px 0px 0px 0px round 0px)";
    animation = animateMini(
      searchRootRef.value,
      { clipPath: show ? [collapsed, full] : [full, collapsed] },
      {
        duration: motionDuration(layerMotion.search),
        ease: layerMotion.ease,
      },
    );
    await animation;
    if (generation === animationGeneration) finishMotion();
  },
  { immediate: true },
);
watch(
  active,
  (show) => {
    if (show) {
      void getSearchHotData();
      nextTick(() => searchRootRef.value?.querySelector("input")?.focus({ preventScroll: true }));
    }
  },
  { immediate: true },
);
watch([inputValue, active], ([value, show]) => {
  clearTimeout(suggestTimer);
  const generation = ++suggestGeneration;
  const query = value.trim();
  if (!query) {
    searchData.suggest = {};
    return;
  }
  if (show) suggestTimer = setTimeout(() => getSearchSuggestData(query, generation), 250);
});
useMotionInterruption(finishMotion);
onMounted(() => {
  document.addEventListener("pointerdown", closeSearchPanel, true);
});
onBeforeUnmount(() => {
  finishMotion();
  clearTimeout(suggestTimer);
  suggestGeneration++;
  document.removeEventListener("pointerdown", closeSearchPanel, true);
});
</script>

<style lang="scss" scoped>
.searchInp {
  position: relative;
  z-index: 1;
  width: clamp(180px, 22vw, 260px);
  display: flex;
  justify-content: flex-start;
  pointer-events: none;
  --search-surface-bg: rgba(var(--app-shell-rgb, 242, 242, 244), 0.58);
  --search-surface-bg-focus: rgba(var(--app-shell-rgb, 242, 242, 244), 0.72);
  --search-dropdown-bg: rgba(var(--app-shell-rgb, 242, 242, 244), 0.78);
  --search-surface-border: var(--acrylic-border, rgba(0, 0, 0, 0.08));
  --search-chip-bg: color-mix(in srgb, var(--content-panel-bg, #fff) 90%, var(--main-color) 10%);
  --search-chip-bg-hover: color-mix(
    in srgb,
    var(--content-panel-bg, #fff) 82%,
    var(--main-color) 18%
  );
  --search-chip-border: color-mix(in srgb, var(--main-color) 24%, transparent);
  --search-chip-text: var(--n-text-color-2, inherit);
  --search-backdrop-filter: blur(18px) saturate(180%);
  --search-dropdown-backdrop-filter: blur(26px) saturate(180%);

  @media (max-width: 450px) {
    width: auto;
  }

  &.active {
    z-index: var(--z-search-overlay, 1900);
  }

  .input {
    --n-color: transparent;
    --n-color-focus: transparent;
    --n-color-hover: transparent;
    --n-border: 1px solid var(--search-surface-border);
    --n-border-hover: 1px solid var(--main-color);
    --n-border-focus: 1px solid var(--main-color);
    --n-box-shadow-focus: 0 0 0 2px var(--main-second-color);
    pointer-events: auto;
    width: 100%;
    height: 32px;
    overflow: hidden;
    background-color: var(--search-surface-bg);
    -webkit-backdrop-filter: var(--search-backdrop-filter);
    backdrop-filter: var(--search-backdrop-filter);
    box-shadow:
      0 8px 24px rgb(0 0 0 / 8%),
      inset 0 0 0 1px var(--acrylic-border, rgba(255, 255, 255, 0.16));
    transition:
      width var(--duration-300) var(--ease-out),
      background-color var(--duration-200) var(--ease-out),
      box-shadow var(--duration-200) var(--ease-out);

    @media (max-width: 450px) {
      width: 36px;
      height: 36px;
      border-radius: var(--radius-pill);
      box-shadow:
        0 8px 20px rgb(0 0 0 / 8%),
        inset 0 0 0 1px var(--acrylic-border, rgba(255, 255, 255, 0.16));
    }

    &.focus {
      width: 100%;
      background-color: var(--search-surface-bg-focus);

      :deep(input) {
        color: var(--main-color);
      }

      @media (max-width: 450px) {
        width: min(54vw, 220px);
        height: 32px;
        border-radius: var(--radius-pill);
      }

      @media (max-width: 380px) {
        width: 50vw;
      }

      @media (max-width: 320px) {
        width: 48vw;
      }
    }

    :deep(.n-input-wrapper) {
      background-color: transparent !important;
      padding-inline: 10px;

      @media (max-width: 450px) {
        padding-inline: 0;
        justify-content: center;
      }
    }

    :deep(.n-input__input-el) {
      background-color: transparent !important;
      height: 32px;
      font-size: 13px;
    }

    :deep(.n-input__input),
    :deep(.n-input__suffix),
    :deep(.n-input__border),
    :deep(.n-input__state-border) {
      background-color: transparent !important;
    }

    :deep(.n-input__prefix) {
      margin-right: 4px;

      @media (max-width: 450px) {
        margin-right: 0;
      }
    }

    :deep(.n-input__prefix) {
      .n-icon {
        transition: color var(--duration-300) var(--ease-out);
        &.active {
          color: var(--main-color);
        }
      }
    }

    @media (max-width: 450px) {
      &:not(.focus) {
        :deep(.n-input__input),
        :deep(.n-input__suffix) {
          display: none;
        }
      }

      &.focus {
        :deep(.n-input-wrapper) {
          padding-inline: 10px;
          justify-content: flex-start;
        }

        :deep(.n-input__prefix) {
          margin-right: 4px;
        }
      }
    }
  }
  .list {
    --n-color: transparent;
    --n-border-color: var(--search-surface-border);
    position: absolute;
    top: calc(var(--app-safe-area-top, 0px) + 38px);
    left: 0;
    border-radius: var(--radius-panel);
    width: 280px;
    z-index: var(--z-search-overlay, 1900);
    pointer-events: auto;
    overflow: hidden;
    background-color: var(--search-dropdown-bg);
    -webkit-backdrop-filter: var(--search-dropdown-backdrop-filter);
    backdrop-filter: var(--search-dropdown-backdrop-filter);
    box-shadow:
      0 18px 46px rgb(0 0 0 / 14%),
      inset 0 0 0 1px var(--acrylic-border, rgba(255, 255, 255, 0.14));

    @media (max-width: 450px) {
      position: fixed;
      width: auto;
      top: calc(var(--app-safe-area-top, 0px) + 56px);
      right: 12px;
      left: 12px;
      border-radius: var(--radius-panel);
      z-index: var(--z-search-overlay, 1900);
    }

    :deep(.n-card__content) {
      background-color: transparent;
    }

    :deep(.n-scrollbar) {
      background-color: transparent;
      max-height: 68vh;
      @media (max-width: 450px) {
        max-height: min(58vh, calc(100vh - var(--app-safe-area-top, 0px) - 148px));
        box-sizing: border-box;
      }
      .n-scrollbar-rail {
        width: 4px;
      }
      .n-scrollbar-container {
        background-color: transparent;
        @media (max-width: 450px) {
          padding-top: 8px;
        }
        .n-scrollbar-content {
          background-color: transparent;
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
                --n-color: var(--search-chip-bg);
                --n-color-hover: var(--search-chip-bg-hover);
                --n-text-color: var(--search-chip-text);
                --n-border: 1px solid var(--search-chip-border);
                background-color: var(--search-chip-bg);
                box-shadow:
                  inset 0 0 0 1px var(--search-chip-border),
                  0 1px 2px rgb(0 0 0 / 4%);
                color: var(--search-chip-text);
                font-size: 12px;
                cursor: pointer;
                transition: all var(--duration-300) var(--ease-out);
                &:hover {
                  background-color: var(--search-chip-bg-hover);
                  box-shadow:
                    inset 0 0 0 1px color-mix(in srgb, var(--main-color) 34%, transparent),
                    0 3px 8px rgb(0 0 0 / 8%);
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
              border-radius: var(--radius-md);
              padding: 5px;
              transition: all var(--duration-300) var(--ease-out);

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
                transition: all var(--duration-300) var(--ease-out);
                border-radius: var(--radius-md);
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

.search-anchor {
  position: relative;
  width: inherit;
  min-width: 0;
}
.search-toolbar {
  display: flex;
  align-items: center;
  width: 100%;
  gap: 8px;
}
.search-results-slot {
  display: contents;
}
.search-cancel {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  flex-shrink: 0;
  min-width: 44px;
  min-height: 44px;
  border: 0;
  padding: 0 8px;
  font: inherit;
  color: var(--main-color);
  cursor: pointer;
  background: transparent;
  border-radius: var(--radius-pill);
  pointer-events: auto;
}
.search-trigger {
  width: 100%;
  cursor: pointer;
  pointer-events: auto;
  .input {
    pointer-events: none;
  }
  &:focus-visible {
    outline: 2px solid var(--main-color);
    outline-offset: 2px;
    border-radius: var(--radius-pill);
  }
}
.searchInp:not(.mobile-search-layer) .search-cancel {
  min-width: 40px;
  min-height: 32px;
  font-size: 12px;
}
.searchInp.mobile-search-layer {
  position: fixed;
  inset: 0;
  z-index: 2200;
  width: 100%;
  height: 100dvh;
  box-sizing: border-box;
  display: flex;
  flex-direction: column;
  overflow: hidden;
  padding: calc(var(--app-safe-area-top, 0px) + 8px) 16px
    max(16px, var(--app-safe-area-bottom, 0px));
  pointer-events: auto;
  background: var(--app-shell-bg, #fff);
  .search-toolbar {
    flex: 0 0 auto;
    min-height: 44px;
    margin-bottom: 12px;
  }
  .input,
  .input.focus {
    width: 100%;
    min-width: 0;
    height: 44px;
    flex: 1;
    box-shadow: none;
  }
  .input :deep(.n-input__input-el) {
    height: 44px;
    font-size: 16px;
  }
  .input :deep(.n-input-wrapper) {
    padding-inline: 12px;
  }
  .list {
    position: relative;
    inset: auto;
    width: 100%;
    flex: 1;
    min-height: 0;
    background: transparent;
    border: 0;
    border-radius: 0;
    box-shadow: none;
    -webkit-backdrop-filter: none;
    backdrop-filter: none;
    :deep(.n-card__content) {
      display: flex;
      flex-direction: column;
      min-height: 0;
    }
    :deep(.n-scrollbar) {
      height: 100%;
      max-height: none;
    }
    :deep(.hot-item),
    :deep(.names) {
      min-height: 44px;
      box-sizing: border-box;
    }
  }
}
</style>
