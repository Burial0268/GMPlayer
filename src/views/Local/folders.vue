<template>
  <LocalGroupGrid
    :groups="local.folders"
    kind="local-folder"
    :fallback-icon="FolderMusic"
    :empty="$t('local.noFolders')"
  />
</template>

<script setup lang="ts">
import { FolderMusic } from "@icon-park/vue-next";
import { useI18n } from "vue-i18n";
import { useLocalLibraryStore } from "@/store";
import LocalGroupGrid from "./LocalGroupGrid.vue";

const { t } = useI18n();
const local = useLocalLibraryStore();

onMounted(() => {
  $setSiteTitle(t("sidebar.localMusic") + " - " + t("local.tab.folders"));
  if (!local.folders.length) local.loadGroups();
});
</script>
