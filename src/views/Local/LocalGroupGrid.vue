<template>
  <div class="local-group-grid">
    <n-empty v-if="!groups.length" :description="empty" size="large" />
    <n-grid v-else :x-gap="20" :y-gap="20" cols="2 s:3 m:4 l:5" responsive="screen">
      <n-gi v-for="group in groups" :key="group.id">
        <div class="group-card" @click="open(group)">
          <div class="cover">
            <!-- `lazy` + `decoding="async"` are load-bearing here, not polish:
                 this grid is not virtualized, so a library with a few thousand
                 albums would otherwise decode every cover the moment the tab
                 opens. The card has a fixed `aspect-ratio`, so the browser knows
                 the box before the image arrives and lazy loading has something
                 to measure. -->
            <img
              v-if="group.coverPath"
              :src="assetUrl(group.coverPath)"
              :alt="group.name"
              loading="lazy"
              decoding="async"
            />
            <n-icon v-else :size="34" :component="fallbackIcon" />
          </div>
          <n-text class="name">{{ group.name || $t("local.unknown") }}</n-text>
          <n-text class="meta" :depth="3">
            {{ subtitleFor(group) }}
          </n-text>
        </div>
      </n-gi>
    </n-grid>
  </div>
</template>

<script setup lang="ts">
import { convertFileSrc } from "@tauri-apps/api/core";
import { useRouter } from "vue-router";
import { useI18n } from "vue-i18n";
import type { Component } from "vue";
import type { LocalGroup } from "@/utils/localLibrary";
import { refToQuery, type PlaylistRef } from "@/utils/playlistSource";

/**
 * The grid behind the albums / artists / folders tabs.
 *
 * Deliberately not `CoverLists.vue`: that component's cards are Netease
 * playlists and albums, complete with `/playlist?id=` links and a play count.
 * A local album has no id anywhere but in this process, so reusing it would mean
 * fabricating one and then intercepting every navigation it produced.
 */
const props = defineProps<{
  groups: LocalGroup[];
  /** Which `PlaylistRef` a card opens. */
  kind: "local-album" | "local-artist" | "local-folder";
  fallbackIcon: Component;
  empty: string;
}>();

const { t } = useI18n();
const router = useRouter();

const assetUrl = (path: string): string => convertFileSrc(path);

const subtitleFor = (group: LocalGroup): string => {
  const count = t("local.trackCount", { count: group.trackCount });
  return group.subtitle ? `${group.subtitle} · ${count}` : count;
};

const refFor = (group: LocalGroup): PlaylistRef => {
  switch (props.kind) {
    case "local-artist":
      return { kind: "local-artist", artist: group.name };
    case "local-folder":
      // `sourceId` / `folder` come straight from Rust rather than being derived
      // from the label: two sources with a subdirectory of the same name are
      // different folders, and the label cannot tell them apart. `folder: ""` is
      // the source's own root, which is why it is a required string.
      return {
        kind: "local-folder",
        sourceId: group.sourceId ?? "",
        folder: group.folder ?? "",
      };
    default:
      return { kind: "local-album", album: group.name };
  }
};

const open = (group: LocalGroup) => {
  router.push({ path: "/local/playlist", query: refToQuery(refFor(group)) });
};
</script>

<style lang="scss" scoped>
.local-group-grid {
  .group-card {
    cursor: pointer;
    .cover {
      position: relative;
      display: flex;
      align-items: center;
      justify-content: center;
      aspect-ratio: 1 / 1;
      border-radius: 8px;
      overflow: hidden;
      background-color: rgba(var(--main-color), 0.12);
      color: rgb(var(--main-color));
      transition: transform var(--duration-300) var(--ease-out);
      img {
        width: 100%;
        height: 100%;
        object-fit: cover;
      }
    }
    .name {
      display: block;
      margin-top: 8px;
      font-size: 15px;
      font-weight: bold;
      // A single line with an ellipsis: two-line titles make the rows in a
      // responsive grid different heights, which reads as broken alignment.
      overflow: hidden;
      text-overflow: ellipsis;
      white-space: nowrap;
    }
    .meta {
      display: block;
      font-size: 12px;
      overflow: hidden;
      text-overflow: ellipsis;
      white-space: nowrap;
    }
    &:hover .cover {
      transform: scale(1.02);
    }
    &:active .cover {
      transform: scale(0.98);
    }
  }
}
</style>
