<template>
  <div v-if="visible" class="mobile-cover-frame" :style="frameStyle"
    @click="$emit('click')">
    <img :src="coverUrl" alt="cover" />
  </div>
</template>

<script setup lang="ts">
defineProps<{
  visible: boolean;
  frameStyle: Record<string, string>;
  coverUrl: string;
}>();

defineEmits<{
  click: [];
}>();
</script>

<style lang="scss" scoped>
.mobile-cover-frame {
  position: absolute;
  width: 0px;
  height: 0px;
  overflow: hidden;
  cursor: pointer;
  pointer-events: auto;
  z-index: 60;
  box-shadow: 0px 12px 40px rgba(0, 0, 0, 0.35);
  transition: width 0.4s ease, height 0.4s ease, left 0.4s ease, top 0.4s ease, border-radius 0.4s ease;

  img {
    position: absolute;
    top: 0;
    left: 0;
    width: 100%;
    height: 100%;
    object-fit: cover;
  }

  &:active {
    opacity: 0.9;
  }
}
</style>
