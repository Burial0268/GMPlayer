<template>
  <Motion
    :class="['bouncing-slider', { indeterminate, leaving }]"
    :style="{ x: bounceXSpring }"
    :onPanStart="handlePanStart"
    :onPan="handlePan"
    :onPanEnd="handlePanEnd"
    :onHoverStart="handleHoverStart"
    :onHoverEnd="handleHoverEnd"
    @click="handleClick"
  >
    <slot name="before-icon" />
    <div ref="innerRef" class="inner-wrapper">
      <Motion tag="div" class="inner" :style="{ clipPath }">
        <div
          v-if="bufferedScale > 0"
          :key="bufferedRun"
          class="buffered"
          :class="{ landed: bufferedScale >= 1 }"
          :style="{ transform: `scaleX(${bufferedScale})` }"
        />
        <Motion tag="div" class="thumb" :style="{ scaleX, originX: 0 }" />
        <div
          class="flow"
          aria-hidden="true"
          @animationend="leaving = false"
          @animationcancel="leaving = false"
        />
      </Motion>
    </div>
    <slot name="after-icon" />
  </Motion>
</template>

<script setup lang="ts">
import {
  Motion,
  useMotionValue,
  useTransform,
  useMotionTemplate,
  useSpring,
  useAnimationFrame,
  animate,
} from "motion-v";
import { computed, ref, watch } from "vue";

const props = withDefaults(
  defineProps<{
    value: number;
    min: number;
    max: number;
    isPlaying?: boolean;
    changeOnDrag?: boolean;
    /** How far the track is buffered, in the units of `value`. */
    buffered?: number;
    /**
     * An AutoMix transition: the bar fills, glows and runs a light forward instead of showing a
     * position, and takes no seeks — there is no single track for one to land in.
     */
    indeterminate?: boolean;
  }>(),
  {
    isPlaying: false,
    changeOnDrag: false,
    indeterminate: false,
  },
);

const emit = defineEmits<{
  "update:value": [val: number];
  seekStart: [];
  seekEnd: [];
}>();

// 0 when unknown, 1 once the whole track has landed. A landed bar is drawn
// transparent rather than removed, so reaching the end fades out instead of
// vanishing, and a local file (landed from the start) never shows one.
const bufferedScale = computed(() => {
  const range = props.max - props.min;
  const buffered = props.buffered ?? 0;
  if (range <= 0 || buffered <= props.min) return 0;
  return Math.min(1, (buffered - props.min) / range);
});

// A new element whenever the buffer moves backwards (next track, a seek past
// what has arrived), so the bar starts where it is instead of sweeping back.
const bufferedRun = ref(0);
watch(bufferedScale, (scale, previous) => {
  if (scale < previous) bufferedRun.value += 1;
});

const MAX_HEIGHT = 20;
const MIN_HEIGHT = 8;
const INITIAL_INSET = (MAX_HEIGHT - MIN_HEIGHT) / 2;
const MAX_BOUNCE_DISTANCE = 12;

// Seek animation easing config
const SEEK_EASING: [number, number, number, number] = [0.4, 0, 0.2, 1];
const SEEK_DURATION = 0.3;

const innerRef = ref<HTMLDivElement | null>(null);

let rectCache: { left: number; width: number } | null = null;
let isHovering = false;
let isDragging = false;
let localTime = props.value;
// Suppress click after drag — panEnd fires before click in the event sequence
let lastPanEndTime = 0;
// Track active seek animation so useAnimationFrame doesn't fight it
let seekAnimControl: ReturnType<typeof animate> | null = null;

// Motion values
const progressMv = useMotionValue(0);
const scaleX = useTransform(progressMv, [0, 1], [0, 1]);

const insetMv = useMotionValue(INITIAL_INSET);
const clipPath = useMotionTemplate`inset(${insetMv}px 0px round 100px)`;

const bounceXSpring = useSpring(0, { damping: 12, stiffness: 300 });

// Leaving the transition the full bar fades down to the next song's segment, which then flashes and
// settles, instead of the fill shrinking back from the end. Cleared when that sequence ends.
const leaving = ref(false);

const progressOf = (time: number) => {
  const range = props.max - props.min;
  return range > 0 ? Math.max(0, Math.min(1, (time - props.min) / range)) : 0;
};

watch(
  () => props.indeterminate,
  (now, was) => {
    leaving.value = was && !now;
    if (!leaving.value) return;
    // The jump to the new song is not a seek: show where it is, under the leaving bar.
    seekAnimControl?.stop();
    seekAnimControl = null;
    progressMv.set(progressOf(localTime));
  },
);

// Animate progress to a target value with bezier easing (for seeks)
const animateProgressTo = (p: number) => {
  seekAnimControl?.stop();
  seekAnimControl = animate(progressMv, p, {
    type: "tween",
    ease: SEEK_EASING,
    duration: SEEK_DURATION,
    onComplete: () => {
      seekAnimControl = null;
    },
  });
};

// Sync external value → progress
watch(
  () => [props.value, props.min, props.max],
  ([val, min, max], oldArr) => {
    if (isDragging) return;
    const v = val as number;
    const mn = min as number;
    const mx = max as number;
    localTime = v;
    const range = mx - mn;
    const p = range > 0 ? Math.max(0, Math.min(1, (v - mn) / range)) : 0;

    // Detect a seek: value jumped by more than 1.5s or min/max changed
    const oldVal = oldArr ? (oldArr[0] as number) : v;
    const jumped = Math.abs(v - oldVal) > 1.5;
    if (jumped && !leaving.value) {
      animateProgressTo(p);
    } else {
      // Normal playback tick — set directly (no animation)
      if (!seekAnimControl) {
        progressMv.set(p);
      }
    }
  },
  { immediate: true },
);

// Frame-interpolated progress while playing
// delta is in milliseconds (from performance.now() diffs), convert to seconds
useAnimationFrame((_time: number, delta: number) => {
  if (props.isPlaying && !isDragging && !seekAnimControl) {
    localTime += delta / 1000;
    if (localTime > props.max) localTime = props.max;
    const range = props.max - props.min;
    const p = range > 0 ? Math.max(0, Math.min(1, (localTime - props.min) / range)) : 0;
    progressMv.set(p);
  }
});

// Expand / collapse
const expand = () => {
  animate(insetMv, 0, { type: "tween", ease: "easeOut", duration: 0.28 });
};

const collapse = () => {
  animate(insetMv, INITIAL_INSET, {
    type: "spring",
    damping: 12,
    stiffness: 200,
  });
};

// Pan handlers
const handlePanStart = () => {
  isDragging = true;
  seekAnimControl?.stop();
  seekAnimControl = null;
  if (innerRef.value) {
    const r = innerRef.value.getBoundingClientRect();
    // info.point uses pageX, so convert rect.left to page coords
    rectCache = { left: r.left + window.scrollX, width: r.width };
  }
  expand();
  emit("seekStart");
};

const handlePan = (_event: Event, info: { point: { x: number } }) => {
  const rect = rectCache;
  if (!rect) return;

  const relPos = (info.point.x - rect.left) / rect.width;

  if (relPos < 0) {
    bounceXSpring.set(Math.tanh(relPos * 2) * MAX_BOUNCE_DISTANCE);
  } else if (relPos > 1) {
    bounceXSpring.set(Math.tanh((relPos - 1) * 2) * MAX_BOUNCE_DISTANCE);
  } else {
    bounceXSpring.set(0);
  }

  const clamped = Math.max(0, Math.min(1, relPos));
  const newValue = props.min + clamped * (props.max - props.min);
  localTime = newValue;
  progressMv.set(clamped);

  if (props.changeOnDrag) {
    emit("update:value", newValue);
  }
};

const handlePanEnd = () => {
  isDragging = false;
  rectCache = null;
  lastPanEndTime = performance.now();

  if (isHovering) {
    expand();
  } else {
    collapse();
  }

  bounceXSpring.set(0);
  emit("update:value", localTime);
  emit("seekEnd");
};

// Hover handlers
const handleHoverStart = () => {
  isHovering = true;
  if (!isDragging) expand();
};

const handleHoverEnd = () => {
  isHovering = false;
  if (!isDragging) collapse();
};

// Click (tap) handler
const handleClick = (event: MouseEvent) => {
  // Suppress click that fires right after panEnd (pointerup → click sequence)
  if (performance.now() - lastPanEndTime < 100) return;

  const rect = innerRef.value?.getBoundingClientRect();
  if (!rect) return;

  const relPos = Math.max(0, Math.min(1, (event.clientX - rect.left) / rect.width));
  const newValue = props.min + relPos * (props.max - props.min);
  localTime = newValue;

  // Animate to the clicked position with bezier easing
  animateProgressTo(relPos);

  emit("seekStart");
  emit("update:value", newValue);
  emit("seekEnd");
};
</script>

<style scoped>
.bouncing-slider {
  touch-action: none;
  cursor: pointer;
  display: flex;
  justify-content: stretch;
  align-items: center;
  gap: var(--bouncing-slider-icon-gap, 10px);
  width: 100%;
  min-height: 24px;
  transform: translateZ(0);
}

.bouncing-slider :deep(.n-icon) {
  width: var(--bouncing-slider-icon-size, 22px) !important;
  height: var(--bouncing-slider-icon-size, 22px) !important;
  font-size: var(--bouncing-slider-icon-size, 22px) !important;
  flex-shrink: 0;
}

.inner-wrapper {
  flex: 1;
  width: 100%;
  /* On the wrapper: `.inner` is clipped to its rounded shape and would clip its own shadow. */
  transition: filter 0.8s ease;
}

.indeterminate {
  pointer-events: none;
}

.indeterminate .inner-wrapper {
  filter: drop-shadow(0 0 6px rgb(255 255 255 / 55%));
  transition-duration: 0.7s;
}

.inner {
  position: relative;
  width: 100%;
  height: 20px;
  background-color: #ffffff26;
}

.buffered,
.thumb {
  position: absolute;
  inset: 0;
  background-color: white;
  transform-origin: left center;
}

.buffered {
  opacity: 0.18;
  /* The backend reports in half-second steps; glide between them. */
  transition:
    transform 0.4s linear,
    opacity 0.7s ease;
}

/* Finish the last stretch of the glide, then dissolve into the rail. */
.buffered.landed {
  opacity: 0;
  transition-delay: 0s, 0.4s;
}

.thumb {
  opacity: 0.4;
  transition: opacity 0.25s ease;
}

.indeterminate .thumb,
.indeterminate .buffered {
  opacity: 0;
}

/* The full bar of the transition state; idle and invisible otherwise. */
.flow {
  position: absolute;
  inset: 0;
  opacity: 0;
  /* A bright band on the filled bar, tiled so the loop has no seam; moving the background one
     tile per cycle carries it forward at a constant speed. */
  background-image: linear-gradient(
    90deg,
    rgb(255 255 255 / 60%) 0%,
    rgb(255 255 255 / 60%) 30%,
    rgb(255 255 255 / 100%) 50%,
    rgb(255 255 255 / 60%) 70%,
    rgb(255 255 255 / 60%) 100%
  );
  background-size: 200% 100%;
}

.indeterminate .flow {
  opacity: 1;
  transition: opacity 0.25s ease;
  animation: slider-flow 2s linear infinite;
}

/* One timeline for both, so the fade's end clears the state only once the thumb has settled. */
.leaving .flow {
  opacity: 1;
  /* The light keeps running: an animation still listed carries on rather than restarting. */
  animation:
    slider-flow 2s linear infinite,
    slider-leave-flow 1.5s forwards;
}

/* Bright under the fading bar, so the next song's segment is what stays behind; then a flash, then
   back to the resting 0.4. A stale full-length thumb for a frame or two is hidden by the bar. */
.leaving .thumb {
  animation: slider-leave-thumb 1.5s forwards;
}

@keyframes slider-leave-flow {
  0% {
    opacity: 1;
    animation-timing-function: ease;
  }

  47%,
  100% {
    opacity: 0;
  }
}

@keyframes slider-leave-thumb {
  0%,
  47% {
    opacity: 0.9;
    animation-timing-function: ease-out;
  }

  60% {
    opacity: 1;
    animation-timing-function: ease-in-out;
  }

  100% {
    opacity: 0.4;
  }
}

@keyframes slider-flow {
  from {
    background-position: 200% 0;
  }

  to {
    background-position: 0 0;
  }
}

@media (prefers-reduced-motion: reduce) {
  .indeterminate .flow {
    animation: none;
  }

  .leaving .flow {
    animation: slider-leave-flow 1.5s forwards;
  }

  .leaving .thumb {
    animation: none;
  }
}

.bouncing-slider > :deep(.n-icon) {
  opacity: 0.5;
  flex-shrink: 0;
}
</style>
