<template>
  <Motion
    class="bouncing-slider"
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
        <Motion tag="div" class="thumb" :style="{ scaleX, originX: 0 }" />
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
import { ref, watch } from "vue";

const props = withDefaults(
  defineProps<{
    value: number;
    min: number;
    max: number;
    isPlaying?: boolean;
    changeOnDrag?: boolean;
  }>(),
  {
    isPlaying: false,
    changeOnDrag: false,
  },
);

const emit = defineEmits<{
  "update:value": [val: number];
  seekStart: [];
  seekEnd: [];
}>();

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
    if (jumped) {
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
  gap: 8px;
  width: 100%;
  min-height: 24px;
  transform: translateZ(0);
}

.inner-wrapper {
  flex: 1;
  width: 100%;
}

.inner {
  width: 100%;
  height: 20px;
  background-color: #ffffff26;
}

.thumb {
  width: 100%;
  height: 100%;
  background-color: white;
  opacity: 0.4;
  transform-origin: left center;
}

.bouncing-slider > :deep(.n-icon) {
  opacity: 0.5;
  flex-shrink: 0;
}
</style>
