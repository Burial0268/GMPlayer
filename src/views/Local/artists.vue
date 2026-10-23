<template>
  <LocalGroupGrid
    :groups="local.artists"
    kind="local-artist"
    :fallback-icon="Voice"
    :empty="$t('local.noArtists')"
  />
</template>

<script setup lang="ts">
import { Voice } from "@icon-park/vue-next";
import { useI18n } from "vue-i18n";
import { useLocalLibraryStore } from "@/store";
import LocalGroupGrid from "./LocalGroupGrid.vue";

const { t } = useI18n();
const local = useLocalLibraryStore();

onMounted(() => {
  $setSiteTitle(t("sidebar.localMusic") + " - " + t("local.tab.artists"));
  if (!local.artists.length) local.loadGroups();
});
</script>
