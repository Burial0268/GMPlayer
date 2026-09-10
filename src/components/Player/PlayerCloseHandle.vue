<template>
  <div class="player-close-handle amll-close-action">
    <ControlThumb :aria-label="ariaLabel" @click="closeBigPlayer" />
  </div>
</template>

<script setup lang="ts">
import ControlThumb from "./ControlThumb.vue";
import { useLayerNavigation } from "@/utils/navigation";

withDefaults(
  defineProps<{
    ariaLabel?: string;
  }>(),
  {
    ariaLabel: "Close player",
  },
);

const navigation = useLayerNavigation();
const closeBigPlayer = () => navigation.closeTop();
</script>

<style lang="scss" scoped>
// 关闭手柄挂在共享的 .left 列上（不再各自内嵌于封面/唱片/评论舞台）。
// 具体坐标由 DesktopPlayerLayout 实时测量封面/唱片舞台位置后，通过
// --close-handle-x / --close-handle-y 注入，从而在封面 ↔ 评论切换时始终
// 落在「封面上方 1.5rem」那一点、切换不跳位、全屏也精确对齐。
.player-close-handle {
  position: absolute;
  left: var(--close-handle-x, 50%);
  top: var(--close-handle-y, 4rem);
  width: 0;
  height: 0;
  z-index: 3;
  color: var(--main-cover-color, rgb(255 255 255 / 0.72));
  mix-blend-mode: plus-lighter;
}
</style>
