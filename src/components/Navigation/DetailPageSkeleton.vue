<template>
  <div class="detail-skeleton" :class="kind" aria-busy="true">
    <div class="detail-header">
      <div class="detail-cover" data-navigation-cover="page">
        <img
          v-if="preview?.image"
          :src="preview.image"
          :data-navigation-shared-image="kind === 'artist' ? '' : undefined"
          alt=""
        />
        <n-skeleton v-else :sharp="false" width="100%" height="100%" />
      </div>
      <div class="detail-meta">
        <span class="detail-kind">{{
          $t(kind === "artist" ? "general.name.artists" : `general.name.${kind}`)
        }}</span>
        <span v-if="preview?.title" class="detail-name" data-navigation-title="page">{{
          preview.title.text
        }}</span>
        <n-skeleton v-else class="detail-name-placeholder" :sharp="false" />
        <n-skeleton class="creator-placeholder" width="160px" height="20px" />
        <n-skeleton class="stats-placeholder" width="220px" height="16px" />
        <n-skeleton text :repeat="2" />
        <div class="actions-placeholder">
          <n-skeleton :sharp="false" width="112px" height="38px" />
          <n-skeleton :sharp="false" width="38px" height="38px" />
        </div>
      </div>
    </div>
    <div class="detail-rows">
      <div v-for="row in 8" :key="row" class="detail-row">
        <n-skeleton :sharp="false" width="42px" height="42px" />
        <div class="row-text"><n-skeleton text width="70%" /><n-skeleton text width="44%" /></div>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { inject } from "vue";
import { pageSourceKey } from "@/utils/navigation/sources";

defineProps<{ kind: "album" | "playlist" | "artist" }>();
const preview = inject(pageSourceKey, undefined);
</script>

<style scoped lang="scss">
@use "@/style/detail-artwork" as artwork;

.detail-skeleton {
  display: flex;
  flex-direction: column;
  gap: 22px;
  padding: 10px clamp(16px, 3vw, 36px) 36px;
}
.detail-header {
  display: grid;
  grid-template-columns: minmax(176px, 278px) minmax(0, 1fr);
  align-items: center;
  gap: clamp(22px, 4vw, 38px);
  padding: 18px 2px 10px;
}
.detail-cover {
  width: 100%;
  aspect-ratio: 1;
  border-radius: var(--radius-md);
  overflow: hidden;
  img {
    display: block;
    width: 100%;
    height: 100%;
    object-fit: cover;
  }
}
.detail-skeleton.artist .detail-cover {
  overflow: visible;
  border-radius: 0;
  img {
    border-radius: var(--radius-md);
  }
}
.detail-meta {
  display: flex;
  flex-direction: column;
  min-width: 0;
}
.detail-kind {
  margin-bottom: 7px;
  color: var(--n-text-color-3);
  font-size: 11px;
  font-weight: 700;
  line-height: 1;
}
.detail-name {
  display: -webkit-box;
  overflow: hidden;
  overflow-wrap: anywhere;
  -webkit-box-orient: vertical;
  -webkit-line-clamp: 2;
  line-clamp: 2;
  font-size: clamp(32px, 5vw, 56px);
  font-weight: 800;
  line-height: normal;
}
.detail-name-placeholder {
  width: 80%;
  height: 48px;
}
.creator-placeholder {
  margin-top: 10px;
}
.stats-placeholder {
  margin-block: 18px;
}
.actions-placeholder {
  display: flex;
  gap: 14px;
  margin-top: 16px;
}
.detail-rows {
  overflow: hidden;
  border-radius: var(--radius-md);
}
.detail-row {
  display: flex;
  align-items: center;
  gap: 14px;
  padding: 9px 6px;
  min-height: 58px;
  &:nth-child(odd) {
    background: color-mix(in srgb, var(--n-text-color) 3%, transparent);
  }
  &:nth-child(even) {
    background: color-mix(in srgb, var(--n-text-color) 6%, transparent);
  }
}
.row-text {
  flex: 1;
  min-width: 0;
}
@media (max-width: 768px) {
  .detail-skeleton {
    gap: 14px;
    padding: 8px 14px 28px;
  }
  .detail-header {
    grid-template-columns: 1fr;
    align-items: start;
    gap: 16px;
    padding: 12px 0 18px;
  }
  .detail-cover {
    justify-self: center;
    width: min(58vw, 260px);
  }
  .detail-name {
    font-size: clamp(25px, 8vw, 36px);
    line-height: normal;
  }
  .detail-name-placeholder {
    height: 35px;
  }
  .creator-placeholder {
    margin-top: 9px;
  }
}
@media (max-width: 540px) {
  .detail-cover {
    width: min(64vw, 235px);
  }
}

.detail-skeleton.album .detail-header {
  @include artwork.album(".detail-cover");
}
</style>
