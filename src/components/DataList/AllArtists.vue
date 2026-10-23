<template>
  <div class="artists">
    <span class="artist" v-for="(item, index) in artistsData.filter((v) => v)" :key="item">
      <n-text
        class="name"
        :depth="isDark ? 3 : 0"
        v-html="item.name"
        role="link"
        tabindex="0"
        @keydown.enter.stop="jumpArtist(item.id, $event)"
        @click.stop="jumpArtist(item.id, $event)"
      />
      <span
        class="line"
        v-if="index != artistsData.length - 1 && artistsData[artistsData.length - 1]"
        >/</span
      >
    </span>
  </div>
</template>

<script setup>
import { useLayerNavigation } from "@/utils/navigation";
const navigation = useLayerNavigation();
const props = defineProps({
  // 歌手数据
  artistsData: {
    type: Array,
    default: [],
  },
  // 是否变灰
  isDark: {
    type: Boolean,
    default: true,
  },
});

// 跳转歌手页面
const jumpArtist = (id, origin) => {
  // 本地曲目的歌手来自文件标签，没有网易 id（`localLibrary` 填的是 0）。
  // 不挡住的话点一下会跳到 `/artist/songs?id=0`，那个页面照常发请求、照常空着。
  const artistId = Number(id);
  if (!Number.isFinite(artistId) || artistId <= 0) return;
  navigation.openPage(
    {
      path: "/artist/songs",
      query: {
        id,
      },
    },
    { origin },
  );
};
</script>

<style lang="scss" scoped>
.artists {
  display: flex;
  flex-direction: row;
  align-items: center;
  flex-wrap: wrap;
  .name {
    cursor: pointer;
    transition: color var(--duration-300) var(--ease-out);
    &:hover {
      color: var(--main-color);
    }
  }
  .line {
    margin: 0 4px;
    opacity: 0.8;
  }
}
</style>
