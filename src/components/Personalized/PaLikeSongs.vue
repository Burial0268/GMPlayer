<template>
  <article
    class="liked-card"
    role="link"
    tabindex="0"
    @click="toLikeSongs"
    @keydown.enter="toLikeSongs"
    @keydown.space.prevent="toLikeSongs"
  >
    <div class="liked-card__artwork" aria-hidden="true">
      <img :src="cardImage" alt="" loading="lazy" @error="useFallbackCover" />
      <span class="liked-card__artwork-icon">♥</span>
    </div>
    <div class="liked-card__content">
      <span class="liked-card__eyebrow">{{ $t("home.modules.likeSong.eyebrow") }}</span>
      <h3>{{ $t("home.modules.likeSong.title") }}</h3>
      <p>{{ $t("home.modules.likeSong.subtitle") }}</p>
    </div>
    <span class="liked-card__arrow" aria-hidden="true">
      <n-icon :component="Right" size="18" />
    </span>
  </article>
</template>

<script setup>
import { Right } from "@icon-park/vue-next";
import { computed, onMounted } from "vue";
import { useI18n } from "vue-i18n";
import { useRouter } from "vue-router";
import { userStore } from "@/store";

const FALLBACK_COVER = "/images/pic/pic.jpg";
const router = useRouter();
const user = userStore();
const { t } = useI18n();

const cardImage = computed(() => {
  const cover = user.getUserPlayLists.own[0]?.cover;
  return cover ? `${cover.replace(/^http:/, "https:")}?param=240y240` : FALLBACK_COVER;
});

const useFallbackCover = (event) => {
  const image = event.currentTarget;
  if (image instanceof HTMLImageElement && !image.src.endsWith(FALLBACK_COVER)) {
    image.src = FALLBACK_COVER;
  }
};

const toLikeSongs = () => {
  if (!user.userLogin) {
    $message.error(t("general.message.needLogin"));
    router.push("/login");
    return;
  }

  const id = user.getUserPlayLists.own[0]?.id;
  if (id) router.push(`/playlist?id=${id}&page=1`);
};

onMounted(() => {
  if (user.userLogin && !user.getUserPlayLists.has && !user.getUserPlayLists.isLoading) {
    user.setUserPlayLists();
  }
});
</script>

<style lang="scss" scoped>
.liked-card {
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

.liked-card__artwork {
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

  &::after {
    content: "";
    position: absolute;
    inset: 0;
    background: linear-gradient(135deg, transparent 38%, rgba(0, 0, 0, 0.38));
  }
}

.liked-card__artwork-icon {
  position: absolute;
  right: 8px;
  bottom: 6px;
  z-index: 1;
  color: #fff;
  font-size: 18px;
  text-shadow: 0 2px 8px rgba(0, 0, 0, 0.28);
}

.liked-card__content {
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

.liked-card__eyebrow {
  color: var(--n-text-color-3);
  font-size: 9px;
  font-weight: 750;
  letter-spacing: 0.11em;
  text-transform: uppercase;
}

.liked-card__arrow {
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

.liked-card:hover,
.liked-card:focus-visible {
  background: color-mix(in srgb, var(--n-text-color) 8%, transparent);

  .liked-card__artwork img {
    transform: scale(1.04);
  }

  .liked-card__arrow {
    color: var(--n-text-color);
    transform: translateX(2px);
  }
}

@container (max-width: 480px) {
  .liked-card {
    grid-template-columns: clamp(58px, 19cqi, 68px) minmax(0, 1fr) 32px;
    gap: 11px;
    height: auto;
    min-height: 88px;
    padding: 10px;
  }

  .liked-card__artwork {
    width: clamp(58px, 19cqi, 68px);
    height: auto;
  }

  .liked-card__content p {
    display: none;
  }

  .liked-card__arrow {
    width: 32px;
    height: 32px;
  }
}

@container (max-width: 360px) {
  .liked-card {
    grid-template-columns: 58px minmax(0, 1fr);
    gap: 10px;
    min-height: 78px;
    padding: 9px;
  }

  .liked-card__artwork {
    width: 58px;
  }

  .liked-card__content h3 {
    margin-top: 2px;
    font-size: 15px;
  }

  .liked-card__eyebrow {
    font-size: 8px;
  }

  .liked-card__arrow {
    display: none;
  }
}

@media (prefers-reduced-motion: reduce) {
  .liked-card,
  .liked-card__artwork img,
  .liked-card__arrow {
    transition: none;
  }
}
</style>
