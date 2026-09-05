<template>
  <div :class="['immersive-layout', { 'no-lyrics': !showLyrics, opened: music.showBigPlayer }]">
    <!-- 全幅封面：铺满左栏（上下出血 + object-fit: cover），右缘按椭圆弧线融进播放背景。
         无歌词时整窗铺满。几何只随「有无歌词」变化，切歌只做纯交叉淡入。 -->
    <div :class="['immersive-artwork', { dimmed: commentsOpen }]" aria-hidden="true">
      <Transition name="immersive-art">
        <img :key="coverUrl" class="immersive-artwork-image" :src="coverUrl" alt="" />
      </Transition>
      <div class="immersive-artwork-veil">
        <Transition name="immersive-art">
          <img :key="coverUrl" class="veil-image" :src="coverUrl" alt="" />
        </Transition>
      </div>
    </div>

    <!-- 评论面板复用桌面版，沿用它自身的 plus-lighter 观感，占位与封面同侧。
         必须走 LayoutGroup + AnimatePresence（和 DesktopPlayerLayout 一致）：面板内部是
         带 layout-id 的 <Motion>，用原生 <Transition> 卸载会让 motion-v 的退场记账
         打到不存在的 presence 上下文上，抛 onMotionExitComplete is not a function。 -->
    <LayoutGroup id="immersive-player-content">
      <AnimatePresence :initial="false">
        <Motion
          v-if="commentsOpen"
          key="comments"
          class="immersive-comment-stage"
          :initial="{ opacity: 0, x: -18 }"
          :animate="{ opacity: 1, x: 0 }"
          :exit="{ opacity: 0, x: -12 }"
          :transition="commentStageTransition"
        >
          <DesktopCommentPanel @close="$emit('closeComments')" />
        </Motion>
      </AnimatePresence>
    </LayoutGroup>

    <div :class="['immersive-lyrics', { 'lyrics-hidden': !showLyrics }]" :aria-hidden="!showLyrics">
      <DesktopLyricsPanel
        :menuShow="menuShow"
        :handleProgressSeek="handleProgressSeek"
        @lrcMouseEnter="$emit('lrcMouseEnter')"
        @lrcAllLeave="$emit('lrcAllLeave')"
        @lrcTextClick="$emit('lrcTextClick', $event)"
      />
    </div>

    <ImmersiveDock
      :class="['immersive-dock-slot', { dimmed: commentsOpen }]"
      :aria-hidden="commentsOpen"
      :handleProgressSeek="handleProgressSeek"
      @openComments="$emit('openComments')"
    />

    <PlayerCloseHandle />
  </div>
</template>

<script setup lang="ts">
import { computed } from "vue";
import { AnimatePresence, LayoutGroup, Motion } from "motion-v";
import { musicStore } from "@/store";
// Aliased: this file already exposes a `coverUrl` computed of its own.
import { coverUrl as toCoverUrl } from "@/utils/coverUrl";
import DesktopCommentPanel from "./DesktopCommentPanel.vue";
import DesktopLyricsPanel from "./DesktopLyricsPanel.vue";
import ImmersiveDock from "./ImmersiveDock.vue";
import PlayerCloseHandle from "../PlayerCloseHandle.vue";

defineProps<{
  menuShow: boolean;
  showLyrics: boolean;
  commentsOpen: boolean;
  handleProgressSeek: (val: number) => void;
}>();

defineEmits<{
  lrcMouseEnter: [];
  lrcAllLeave: [];
  lrcTextClick: [time: number];
  openComments: [];
  closeComments: [];
}>();

const music = musicStore();

// 与 DesktopPlayerLayout 的 stageSpring 同参：评论面板在两种模式下入场手感一致
const commentStageSpring = {
  type: "spring",
  stiffness: 180,
  damping: 42,
  mass: 1.35,
  restDelta: 0.001,
  restSpeed: 0.01,
} as const;
const commentStageTransition = {
  ...commentStageSpring,
  default: commentStageSpring,
  opacity: { type: "tween", duration: 0.62, ease: [0.16, 1, 0.3, 1], delay: 0.1 },
} as const;

const coverUrl = computed(() => toCoverUrl(music.getPlaySongData?.album?.picUrl, 1024));
</script>

<style lang="scss" scoped>
// mask-image 本身不可过渡，但注册过的自定义属性可以。把椭圆的横向半径注册成
// <percentage>，歌词有无切换时封面宽度与淡出边缘就能作为同一次动作一起插值，
// 而不是宽度滑、边缘跳。
@property --immersive-art-fade-rx {
  syntax: "<percentage>";
  inherits: true;
  initial-value: 100%;
}

.immersive-layout {
  position: absolute;
  inset: 0;
  min-width: 0;
  min-height: 0;

  // 一律用 vw 且不设上限：宽屏下拖大窗口时封面栏、控制卡都要继续跟着长，
  // 卡在 clamp 的 max 上就等于不 responsive。
  --immersive-art-width: max(16rem, 52vw);
  --immersive-art-fade-rx: 100%;
  // 控制卡宽度不设上下限，纯粹跟着窗口走；卡片内部的字号/图标也按 vw 缩放
  // （见 ImmersiveDock），所以窄窗口下不会挤爆、宽屏下也不会钉住不动。
  --immersive-dock-width: 27vw;
  // 封面、关闭手柄、控制卡三者的对齐完全交给 CSS 引擎：锚点就是封面栏的水平中点
  // （object-fit: cover 默认 50% 居中，画面中轴即盒子中轴；右缘的椭圆渐隐只是蒙纱、
  // 没有裁掉画面，所以不能按「实心区中点」取值）。
  //
  // 这里必须是纯 vw/rem 的 calc —— 不要退回 JS 实测 + 写 inline 变量：
  // 自定义属性是继承的，往根节点写一次就会让整棵子树重新算样式，控制卡的
  // backdrop-filter 随之重新栅格化；别处 hover 动画引发的重算会不断触发这条链路，
  // 表现为 GPU 绘制 glitch。CSS 变量在窗口尺寸变化时由引擎自己重解析，零 JS。
  --immersive-anchor-x: calc(var(--immersive-art-width) / 2);
  --close-handle-x: var(--immersive-anchor-x);
  --close-handle-y: calc(var(--app-safe-area-top, 0px) + 2.5rem);
  // 控制卡的定位也提到变量里：ImmersiveDock 内部要拿它反推「假玻璃」那张模糊副本
  // 相对封面的偏移（见 .dock-glass），两边必须同源。
  --immersive-dock-left: max(
    calc(var(--app-safe-area-left, 0px) + 1rem),
    calc(var(--immersive-anchor-x) - var(--immersive-dock-width) / 2)
  );
  --immersive-dock-bottom: calc(var(--app-safe-area-bottom, 0px) + max(1rem, 3.2vh));

  // ── 动效令牌 ──────────────────────────────────────────────
  // 沉浸模式里同时发生的动作很多，按「这次动作意味着什么」分组共用时长，
  // 而不是每处各调各的 —— 一致性正是这套界面读起来整不整的关键。
  // 曲线统一用 --ease-out（= cubic-bezier(0.16, 1, 0.3, 1)）：几乎全程减速，
  // 元素是「滑到位」而不是「撞到位」。
  --im-ease: var(--ease-out);
  --im-quick: 0.24s; // 让位：被其他内容顶掉时迅速退开
  --im-layout: 0.42s; // 布局重排：歌词开关，封面/歌词/控制卡/手柄同时改位
  --im-content: 0.55s; // 内容替换：切歌交叉淡入
  --im-reveal: 0.62s; // 开合入场
  --im-step: 0.06s; // 分层入场的层间间隔

  transition: --immersive-art-fade-rx var(--im-layout) var(--im-ease);
}

// 无歌词：封面铺满整窗，而不是留半屏空背景。淡出边缘随之退到画面之外
// （fade-rx 变大 = 椭圆实心核覆盖整个元素），锚点也随宽度一起移到正中。
.immersive-layout.no-lyrics {
  --immersive-art-width: 100vw;
  --immersive-art-fade-rx: 210%;
}

/* ═══════════ 全幅封面 ═══════════ */
.immersive-artwork {
  position: absolute;
  inset: 0 auto 0 0;
  width: var(--immersive-art-width);
  overflow: hidden;
  pointer-events: none;
  // 过渡边缘是椭圆而非直线：椭圆锚在左边缘中点，纵向半径远大于元素高度，
  // 因此左、上、下三边整条都保持实心出血，只有右缘按椭圆弧线渐隐——
  // 中段外扩、上下收窄，和参考图的弧形交界一致。
  mask-image: radial-gradient(
    ellipse var(--immersive-art-fade-rx) 140% at 0% 50%,
    rgb(0 0 0 / 1) 0 52%,
    rgb(0 0 0 / 0.55) 76%,
    rgb(0 0 0 / 0) 100%
  );
  -webkit-mask-image: radial-gradient(
    ellipse var(--immersive-art-fade-rx) 140% at 0% 50%,
    rgb(0 0 0 / 1) 0 52%,
    rgb(0 0 0 / 0.55) 76%,
    rgb(0 0 0 / 0) 100%
  );
  transform: scale(var(--immersive-art-scale, 1));
  transition:
    width var(--im-layout) var(--im-ease),
    opacity var(--im-content) var(--im-ease),
    transform var(--im-reveal) var(--im-ease);

  // 评论面板占用左栏时让位。退开走 --im-quick、和控制卡同一条时长，
  // 两者是同一个动作的两半；回来时各自吃基础时长（慢一点），
  // 「快退、缓进」正是让位读起来不生硬的原因。
  &.dimmed {
    opacity: 0;
    transition: opacity var(--im-quick) var(--im-ease);
  }
}

// 上下两条「虚化带」—— 整套沉浸模式的可读性来源。封面顶到窗口四边，而封面本身
// 可能是纯白图，也可能像插画封面那样满是高频细节；关闭手柄、控制卡、右下角开关
// 全是 plus-lighter 白色，只压暗解决不了「字压在花纹上」的噪感，必须把带内的
// 画面本身糊成低频底。（反过来给控件加暗底会变成一块突兀的灰板，和右下角
// 无底的开关不成体系。）
//
// 用「再画一张模糊副本」而不是 backdrop-filter：父级 .immersive-artwork 带 mask，
// 按规范它就是一个 backdrop root，Chromium 在这种嵌套下会把后代的 backdrop-filter
// 当成空 backdrop 处理 —— 结果是一点模糊都看不到。模糊副本是纯粹的合成，稳定可控。
//
// 一层盖满整个封面区，靠自身 mask 只在上下两端显形，中段完全透明露出清晰封面；
// 同时作为子元素继续吃父级的椭圆 mask，所以右缘不会多出一条硬边。
.immersive-artwork-veil {
  position: absolute;
  inset: 0;
  overflow: hidden;
  pointer-events: none;
  mask-image: linear-gradient(
    to bottom,
    rgb(0 0 0 / 1) 0,
    rgb(0 0 0 / 0.5) clamp(80px, 12vh, 165px),
    rgb(0 0 0 / 0) clamp(150px, 22vh, 300px),
    rgb(0 0 0 / 0) calc(100% - clamp(240px, 40vh, 500px)),
    rgb(0 0 0 / 0.55) calc(100% - clamp(120px, 20vh, 250px)),
    rgb(0 0 0 / 1) 100%
  );
  -webkit-mask-image: linear-gradient(
    to bottom,
    rgb(0 0 0 / 1) 0,
    rgb(0 0 0 / 0.5) clamp(80px, 12vh, 165px),
    rgb(0 0 0 / 0) clamp(150px, 22vh, 300px),
    rgb(0 0 0 / 0) calc(100% - clamp(240px, 40vh, 500px)),
    rgb(0 0 0 / 0.55) calc(100% - clamp(120px, 20vh, 250px)),
    rgb(0 0 0 / 1) 100%
  );
}

.veil-image {
  // 比容器大一圈：blur 在图片自身边缘会采样到透明，外扩后这条软边落在裁切之外，
  // 窗口上下沿才不会透出一圈没糊到的清晰封面。
  position: absolute;
  inset: -8%;
  width: auto;
  height: auto;
  display: block;
  object-fit: cover;
  filter: blur(clamp(14px, 2.4vh, 32px));
}

// 压暗压在模糊之上：先糊掉细节，再压低亮度，白色 plus-lighter 控件才浮得起来。
// 用 ::after 而不是真节点 —— 伪元素是盒内最后一项，天然压在绝对定位的
// .veil-image 之上（切歌时同时在场的两张也一样）。
.immersive-artwork-veil::after {
  content: "";
  position: absolute;
  inset: 0;
  background:
    linear-gradient(
      to bottom,
      rgb(0 0 0 / 0.5) 0,
      rgb(0 0 0 / 0.16) clamp(90px, 13vh, 180px),
      rgb(0 0 0 / 0) clamp(160px, 23vh, 310px)
    ),
    linear-gradient(
      to top,
      rgb(0 0 0 / 0.62) 0,
      rgb(0 0 0 / 0.24) clamp(120px, 20vh, 250px),
      rgb(0 0 0 / 0) clamp(250px, 41vh, 510px)
    );
}

.immersive-artwork-image {
  // 切歌时两张封面同时在场做纯不透明度交叉淡入：
  // 绝对定位 + cover 保证两张图完全同框，不产生任何缩放/位移
  position: absolute;
  inset: 0;
  width: 100%;
  height: 100%;
  display: block;
  object-fit: cover;
}

/* ═══════════ 歌词 ═══════════ */
.immersive-lyrics {
  position: absolute;
  inset: 0 0 0 auto;
  // 与封面栏的 52vw 严丝合缝对拼：封面右侧 24% 的椭圆渐隐正好压在歌词左缘之下。
  // 同样不设上限，宽屏拉大时两栏一起长。
  width: max(18rem, 48vw);
  min-width: 0;
  box-sizing: border-box;
  padding-right: calc(var(--app-safe-area-right, 0px) + clamp(1rem, 2.6vw, 3rem));
  z-index: 1;
  overflow: hidden;
  // 与 DesktopPlayerLayout 的 .right 保持一致：歌词以 plus-lighter 与背景混合，
  // 因此本组件根节点不能引入 isolation / filter 之类的层叠隔断。
  mix-blend-mode: plus-lighter;
  opacity: 1;
  visibility: visible;
  // 收起时向右后方缩回而不是单纯平移：原点定在右缘，读起来是「这一栏退出去」，
  // 和封面同步展宽正好是一进一退。时长与封面的 width / 控制卡的 left 同为 0.42s。
  transform-origin: right center;
  transform: translate3d(0, 0, 0) scale(1);
  transition:
    transform var(--im-layout) var(--im-ease),
    opacity var(--im-quick) var(--im-ease) var(--im-step),
    visibility 0s linear;
  will-change: transform, opacity;

  &.lyrics-hidden {
    opacity: 0;
    visibility: hidden;
    transform: translate3d(28px, 0, 0) scale(0.94);
    pointer-events: none;
    transition:
      opacity var(--im-quick) var(--im-ease),
      transform var(--im-layout) var(--im-ease),
      visibility 0s linear var(--im-layout);
  }
}

/* ═══════════ 评论 ═══════════ */
.immersive-comment-stage {
  position: absolute;
  left: 0;
  top: 0;
  bottom: 0;
  width: clamp(22rem, 42%, 40rem);
  box-sizing: border-box;
  padding: calc(var(--app-safe-area-top, 0px) + clamp(72px, 10vh, 112px)) clamp(12px, 1.5vw, 24px)
    calc(var(--app-safe-area-bottom, 0px) + clamp(84px, 11vh, 112px))
    calc(var(--app-safe-area-left, 0px) + clamp(24px, 3vw, 48px));
  // 不设 z-index：靠 DOM 顺序垫在控制卡之下，且不打断控制卡 plus-lighter 到封面的混合链
}

/* ═══════════ 浮动控制卡 ═══════════
   几何全在这里，卡片自身只管排版（width: 100%）。
   横向与关闭手柄共用同一个 CSS 锚点（封面栏中轴）居中对齐，所以上下成一条轴；
   居中用 left - width/2 而不是 translateX(-50%)：常驻 transform 会让本节点成为
   层叠上下文，卡片内部的 plus-lighter 就混不到身下的封面上了（will-change 同理）。
   锚点变量是瞬时切换的，所以 left 跟封面的 width 走同一条 0.42s，三者一起动。 */
.immersive-dock-slot {
  position: absolute;
  left: var(--immersive-dock-left);
  bottom: var(--immersive-dock-bottom);
  width: var(--immersive-dock-width);
  transition:
    left var(--im-layout) var(--im-ease),
    opacity var(--im-reveal) var(--im-ease) calc(var(--im-step) * 2),
    transform var(--im-reveal) var(--im-ease) calc(var(--im-step) * 2);
}

// 手柄的 left 同样吃 --close-handle-x 的瞬时切换，补一条同参过渡跟上封面
:deep(.player-close-handle.amll-close-action) {
  transition: left var(--im-layout) var(--im-ease);
}

// 评论面板接管左栏时和封面一起让位（封面模式下开评论同样是整块换掉封面 + 控件）。
// 单独给一条无延迟的过渡：入场那条带层间 stagger，拿来做让位会慢半拍。
.immersive-dock-slot.dimmed {
  opacity: 0;
  pointer-events: none;
  transition: opacity var(--im-quick) var(--im-ease);
}

/* ═══════════ 封面切歌交叉淡入 ═══════════
   只动 opacity。封面是铺满左栏的出血面板，任何缩放/位移都会在边缘露出背景，
   而且会和入场、评论让位两条过渡叠在一起。 */
.immersive-art-enter-active,
.immersive-art-leave-active {
  transition: opacity var(--im-content) var(--im-ease);
}

.immersive-art-enter-from,
.immersive-art-leave-to {
  opacity: 0;
}

/* ═══════════ 大播放器开合时的分层入场 ═══════════
   BigPlayer 常驻挂载，入场只能由 .opened 状态驱动，不能用挂载期动画。
   封面只做「放大收回」：向内缩会在上下左三边露出背景。 */
.immersive-layout:not(.opened) {
  .immersive-artwork,
  .immersive-lyrics,
  .immersive-dock-slot {
    opacity: 0;
  }

  .immersive-artwork {
    --immersive-art-scale: 1.04;
  }

  .immersive-dock-slot {
    transform: translate3d(0, 16px, 0);
  }
}

// 无歌词时封面就是整个舞台，起手放得更开、收得更慢，落地才有分量。
// 有歌词时封面只是左栏，同样幅度会显得晃，所以只在这里加码。
.immersive-layout.no-lyrics:not(.opened) .immersive-artwork {
  --immersive-art-scale: 1.12;
}

.immersive-layout.opened {
  .immersive-artwork {
    // width 必须一起列出来：这条 transition 会整条覆盖基础规则，
    // 漏了它，开着播放器切歌词时封面宽度会硬跳，而控制卡的 left、
    // 椭圆的 fade-rx 仍在插值，三者就散了。
    transition:
      width var(--im-layout) var(--im-ease),
      opacity var(--im-reveal) var(--im-ease),
      transform var(--im-reveal) var(--im-ease);
  }
}

.immersive-layout.no-lyrics.opened .immersive-artwork {
  transition:
    width var(--im-layout) var(--im-ease),
    opacity var(--im-reveal) var(--im-ease),
    transform calc(var(--im-reveal) * 1.7) var(--im-ease);
}

@media screen and (max-width: 1180px) {
  .immersive-lyrics {
    padding-right: calc(var(--app-safe-area-right, 0px) + 1rem);
  }
}

/* 移动端横屏（isMobile 按宽度判定，横屏手机/平板会落到这套桌面布局上）：
   压缩留白，让浮动卡片和歌词在 ~400px 高的视口里仍然完整可见。 */
@media screen and (max-height: 560px) {
  .immersive-comment-stage {
    padding: calc(var(--app-safe-area-top, 0px) + 46px) 12px
      calc(var(--app-safe-area-bottom, 0px) + 20px) calc(var(--app-safe-area-left, 0px) + 16px);
  }
}
</style>
