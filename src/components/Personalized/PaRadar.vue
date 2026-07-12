<template>
  <article
    class="radar-card"
    role="link"
    tabindex="0"
    @click="openRadar"
    @keydown.enter="openRadar"
    @keydown.space.prevent="openRadar"
  >
    <div class="radar-card__artwork" aria-hidden="true">
      <img src="/images/pic/radar.jpg" alt="" loading="lazy" />
      <span class="radar-card__artwork-icon">
        <n-icon :component="RadarThree" size="19" />
      </span>
    </div>
    <div class="radar-card__content">
      <span class="radar-card__eyebrow">{{ $t("home.modules.radar.eyebrow") }}</span>
      <h3>{{ $t("home.modules.radar.title") }}</h3>
      <p>{{ $t("home.modules.radar.subtitle") }}</p>
    </div>
    <span class="radar-card__arrow" aria-hidden="true">
      <n-icon :component="Right" size="18" />
    </span>
  </article>
</template>

<script setup>
import { RadarThree, Right } from "@icon-park/vue-next";
import { useRouter } from "vue-router";

const router = useRouter();
const radarId = 3136952023;
const openRadar = () => router.push(`/playlist?id=${radarId}&page=1`);
</script>

<style lang="scss" scoped>
.radar-card {
  position: relative;
  isolation: isolate;
  contain: layout paint;
  container-type: inline-size;
  display: grid;
  grid-template-columns: 82px minmax(0, 1fr) 38px;
  align-items: center;
  gap: 15px;
  width: 100%;
  min-width: 0;
  height: 118px;
  min-height: 0;
  padding: 12px 14px 12px 12px;
  overflow: clip;
  box-sizing: border-box;
  border: 1px solid color-mix(in srgb, var(--n-text-color) 8%, transparent);
  border-radius: var(--radius-panel);
  background: color-mix(in srgb, var(--n-text-color) 4%, transparent);
  cursor: pointer;
  outline: none;
  transition: background-color var(--duration-200) var(--ease-out);
}

.radar-card__artwork {
  position: relative;
  width: 82px;
  height: 82px;
  overflow: hidden;
  border-radius: calc(var(--radius-panel) - 5px);
  background: color-mix(in srgb, var(--n-text-color) 7%, transparent);
  aspect-ratio: 1;
  max-width: 100%;

  img {
    position: absolute;
    inset: 0;
    display: block;
    width: 100%;
    height: 100%;
    object-fit: cover;
    transition: transform var(--duration-400) var(--ease-out);
  }
}

.radar-card__artwork-icon {
  position: absolute;
  right: 7px;
  bottom: 7px;
  z-index: 1;
  display: grid;
  place-items: center;
  width: 30px;
  height: 30px;
  color: #fff;
  border-radius: var(--radius-pill);
  background: rgba(12, 12, 12, 0.72);
}

.radar-card__content {
  min-width: 0;

  h3,
  p {
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
  }

  h3 {
    margin: 4px 0 0;
    font-size: 16px;
    font-weight: 700;
    letter-spacing: -0.02em;
  }

  p {
    margin: 3px 0 0;
    color: var(--n-text-color-3);
    font-size: 12px;
  }
}

.radar-card__eyebrow {
  color: var(--n-text-color-3);
  font-size: 9px;
  font-weight: 750;
  letter-spacing: 0.11em;
  text-transform: uppercase;
}

.radar-card__arrow {
  display: grid;
  place-items: center;
  width: 36px;
  height: 36px;
  flex: none;
  color: var(--n-text-color-2);
  border-radius: var(--radius-pill);
  background: color-mix(in srgb, var(--n-text-color) 6%, transparent);
  transition:
    color var(--duration-200) var(--ease-out),
    transform var(--duration-200) var(--ease-out);
}

.radar-card:hover,
.radar-card:focus-visible {
  background: color-mix(in srgb, var(--n-text-color) 8%, transparent);

  .radar-card__artwork img {
    transform: scale(1.04);
  }

  .radar-card__arrow {
    color: var(--n-text-color);
    transform: translateX(2px);
  }
}

@container (max-width: 480px) {
  .radar-card {
    grid-template-columns: clamp(58px, 19cqi, 68px) minmax(0, 1fr) 32px;
    gap: 11px;
    height: auto;
    min-height: 88px;
    padding: 10px;
  }

  .radar-card__artwork {
    width: clamp(58px, 19cqi, 68px);
    height: auto;
  }

  .radar-card__content p {
    display: none;
  }

  .radar-card__arrow {
    width: 32px;
    height: 32px;
  }
}

@container (max-width: 360px) {
  .radar-card {
    grid-template-columns: 58px minmax(0, 1fr);
    gap: 10px;
    min-height: 78px;
    padding: 9px;
  }

  .radar-card__artwork {
    width: 58px;
  }

  .radar-card__content h3 {
    margin-top: 2px;
    font-size: 15px;
  }

  .radar-card__eyebrow {
    font-size: 8px;
  }

  .radar-card__arrow {
    display: none;
  }
}

@media (prefers-reduced-motion: reduce) {
  .radar-card,
  .radar-card__artwork img,
  .radar-card__arrow {
    transition: none;
  }
}
</style>
