<template>
  <div
    ref="containerRef"
    class="control-thumb"
    :class="{ hovering, pressing }"
  >
    <button
      v-bind="attrs"
      type="button"
      :style="{
        '--thumb-x': `${thumbOffset.x}px`,
        '--thumb-y': `${thumbOffset.y}px`,
      }"
      @pointerenter="handlePointerEnter"
      @pointermove="handlePointerMove"
      @pointerleave="handlePointerLeave"
      @pointerdown="pressing = true"
      @pointerup="pressing = false"
      @pointercancel="pressing = false"
      @click="emit('click')"
    >
      <span />
      <span />
    </button>
  </div>
</template>

<script setup lang="ts">
import { reactive, ref, useAttrs } from "vue";

defineOptions({
  inheritAttrs: false,
});

const emit = defineEmits<{
  click: [];
}>();

const attrs = useAttrs();
const containerRef = ref<HTMLElement | null>(null);
const hovering = ref(false);
const pressing = ref(false);
const thumbOffset = reactive({
  x: 0,
  y: 0,
});

const resetThumbOffset = () => {
  thumbOffset.x = 0;
  thumbOffset.y = 0;
};

const handlePointerMove = (event: PointerEvent) => {
  if (!hovering.value) return;
  const container = containerRef.value;
  if (!container) return;

  const button = event.currentTarget;
  if (!(button instanceof HTMLElement)) return;

  const rect = button.getBoundingClientRect();
  const centerX = rect.left + rect.width / 2;
  const centerY = rect.top + rect.height / 2;
  const distanceX = event.clientX - centerX;
  const distanceY = event.clientY - centerY;

  if (Math.abs(distanceX) > 28 || Math.abs(distanceY) > 28) {
    resetThumbOffset();
    return;
  }

  thumbOffset.x = Math.max(-3, Math.min(3, distanceX * 0.18));
  thumbOffset.y = Math.max(-3, Math.min(3, distanceY * 0.18));
};

const handlePointerEnter = () => {
  hovering.value = true;
  resetThumbOffset();
};

const handlePointerLeave = () => {
  hovering.value = false;
  pressing.value = false;
  resetThumbOffset();
};
</script>

<style lang="scss" scoped>
.control-thumb {
  justify-self: center;
  position: relative;
  width: 100%;
  height: 100%;
  color: rgb(255 255 255 / 0.48);
  mix-blend-mode: plus-lighter;

  button {
    display: block;
    touch-action: none;
    position: absolute;
    left: 0;
    top: 0;
    width: 50px;
    height: 8px;
    border: none;
    padding: 0;
    border-radius: 4px;
    background-color: rgb(255 255 255 / 0.12);
    color: inherit;
    cursor: none;
    filter: brightness(0.82);
    transform: translate(
        calc(-50% + var(--thumb-x)),
        calc(-50% + var(--thumb-y))
      )
      scale(var(--thumb-scale, 1));
    transition:
      background-color 0.18s cubic-bezier(0.25, 1, 0.5, 1),
      width 0.18s cubic-bezier(0.25, 1, 0.5, 1),
      height 0.18s cubic-bezier(0.25, 1, 0.5, 1);
    will-change: transform, width, height;

    span {
      position: absolute;
      left: 25px;
      top: 50%;
      width: 0;
      height: 0;
      margin-top: 0;
      background-color: currentColor;
      border-radius: 10px;
      rotate: 0deg;
      transition:
        width 0.18s cubic-bezier(0.25, 1, 0.5, 1),
        height 0.18s cubic-bezier(0.25, 1, 0.5, 1),
        left 0.18s cubic-bezier(0.25, 1, 0.5, 1),
        margin-top 0.18s cubic-bezier(0.25, 1, 0.5, 1),
        rotate 0.18s cubic-bezier(0.25, 1, 0.5, 1);
    }
  }

  &.hovering button {
    width: 25px;
    height: 25px;
    background-color: rgb(255 255 255 / 0.14);

    span {
      left: 5px;
      width: 15px;
      height: 2px;
      margin-top: -1px;

      &:first-child {
        rotate: 45deg;
      }

      &:last-child {
        rotate: -45deg;
      }
    }
  }

  &.pressing button {
    --thumb-scale: 0.96;
  }
}
</style>
