<template>
  <div class="users">
    <PageLoadState v-if="error" error @retry="retry" />
    <UserLists v-else :listData="searchData" :loading="loading" />
    <Pagination
      v-if="searchData[0]"
      :pageNumber="pageNumber"
      :totalCount="totalCount"
      @pageSizeChange="pageSizeChange"
      @pageNumberChange="pageNumberChange"
    />
  </div>
</template>

<script setup lang="ts">
import UserLists from "@/components/DataList/UserLists.vue";
import PageLoadState from "@/components/Navigation/PageLoadState.vue";
import Pagination from "@/components/Pagination/index.vue";
import { useSearchResults } from "@/composables/useSearchResults";

const {
  items: searchData,
  loading,
  error,
  retry,
  totalCount,
  pageNumber,
  pageSizeChange,
  pageNumberChange,
} = useSearchResults({
  category: "users",
  type: 1002,
  items: "userprofiles",
  total: "userprofileCount",
  map: (items) =>
    items.map((item) => ({
      id: item.userId,
      name: item.nickname,
      cover: item.avatarUrl,
      signature: item.signature,
    })),
});
</script>
