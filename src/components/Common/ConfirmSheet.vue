<template>
  <GlassSheet v-model:show="confirmShow" :title="options?.title" :width="400" :z-index="2400">
    <div v-if="options?.content" class="confirm-content">
      <ConfirmContent />
    </div>
    <template #footer>
      <n-button v-if="options?.negativeText" strong secondary round @click="negative">
        {{ options.negativeText }}
      </n-button>
      <n-button
        v-if="options?.positiveText"
        type="primary"
        strong
        secondary
        round
        :loading="pending"
        @click="positive"
      >
        {{ options.positiveText }}
      </n-button>
    </template>
  </GlassSheet>
</template>

<script setup lang="ts">
/**
 * The app's one confirmation sheet; `window.$dialog` opens it (see `utils/ui/confirmSheet`).
 * Above every other sheet (2400): a confirmation can be raised from inside one.
 */
import { computed, ref } from "vue";
import GlassSheet from "@/components/Common/GlassSheet.vue";
import { confirmShow, confirmState } from "@/utils/ui/confirmSheet";

const options = computed(() => confirmState.value);
const pending = ref(false);

const ConfirmContent = () => {
  const content = options.value?.content;
  return typeof content === "function" ? content() : content;
};

const negative = () => {
  confirmShow.value = false;
  options.value?.onNegativeClick?.();
};

const positive = async () => {
  const current = options.value;
  pending.value = true;
  try {
    if ((await current?.onPositiveClick?.()) === false) return;
  } finally {
    pending.value = false;
  }
  // Only close the confirmation this click belonged to, not one opened meanwhile.
  if (confirmState.value === current) confirmShow.value = false;
};
</script>

<style lang="scss" scoped>
.confirm-content {
  padding: 2px 4px 8px;
  line-height: 1.6;
  color: var(--text-secondary);
}
</style>
