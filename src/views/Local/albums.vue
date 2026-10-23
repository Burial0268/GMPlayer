<template>
  <LocalGroupGrid
    :groups="local.albums"
    kind="local-album"
    :fallback-icon="RecordDisc"
    :empty="$t('local.noAlbums')"
  />
</template>

<script setup lang="ts">
import { RecordDisc } from "@icon-park/vue-next";
import { useI18n } from "vue-i18n";
import { useLocalLibraryStore } from "@/store";
import LocalGroupGrid from "./LocalGroupGrid.vue";

const { t } = useI18n();
const local = useLocalLibraryStore();

onMounted(() => {
  $setSiteTitle(t("sidebar.localMusic") + " - " + t("local.tab.albums"));
  if (!local.albums.length) local.loadGroups();
});
</script>
