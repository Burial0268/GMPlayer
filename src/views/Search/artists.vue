<template>
  <div class="artists">
    <PageLoadState v-if="error" error @retry="retry" />
    <ArtistLists v-else :listData="searchData" :loading="loading" />
  </div>
</template>

<script setup lang="ts">
import ArtistLists from "@/components/DataList/ArtistLists.vue";
import PageLoadState from "@/components/Navigation/PageLoadState.vue";
import { useSearchResults } from "@/composables/useSearchResults";

const {
  items: searchData,
  loading,
  error,
  retry,
} = useSearchResults({
  category: "artists",
  type: 100,
  items: "artists",
  total: "artistCount",
  map: (items) => items.map((item) => ({ id: item.id, name: item.name, cover: item.img1v1Url })),
});
</script>
