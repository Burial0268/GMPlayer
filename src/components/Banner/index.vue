<template>
  <div ref="bannerRoot" class="banner-shell">
    <n-skeleton
      v-if="!bannerData[0]"
      :style="{ height: bannerHeight + 'px' }"
      width="100%"
      :sharp="false"
    />
    <Transition>
      <n-carousel
        v-if="bannerData[0]"
        autoplay
        draggable
        keyboard
        class="banner"
        dot-placement="bottom"
        :effect="bannerType"
        :show-arrow="showBannerArrow"
        :show-dots="bannerData.length > 1"
        :style="{ height: bannerHeight + 'px' }"
      >
        <n-carousel-item
          v-for="item in bannerData"
          :key="item"
          class="item"
          :style="bannerType == 'card' ? 'width:60%' : ''"
        >
          <img
            :src="item.imageUrl.replace(/^http:/, 'https:') + '?imageView&quality=89'"
            alt="banner"
            @click="bannerJump(item.targetType, item.targetId, item.url)"
          />
        </n-carousel-item>
        <template #arrow="{ prev, next }">
          <button
            class="banner-arrow banner-arrow--prev"
            type="button"
            :aria-label="$t('home.banner.previous')"
            @click.stop="prev"
          >
            <n-icon size="20" :component="Left" />
          </button>
          <button
            class="banner-arrow banner-arrow--next"
            type="button"
            :aria-label="$t('home.banner.next')"
            @click.stop="next"
          >
            <n-icon size="20" :component="Right" />
          </button>
        </template>
        <template #dots="{ total, currentIndex, to }">
          <div
            class="banner-controls"
            role="group"
            :aria-label="$t('home.banner.controls')"
            :style="getBannerControlsStyle(total)"
          >
            <button
              v-for="index in total"
              :key="index"
              class="banner-control"
              :class="{ 'is-active': currentIndex === index - 1 }"
              type="button"
              :aria-label="$t('home.banner.switchTo', { index })"
              :aria-current="currentIndex === index - 1 ? 'true' : undefined"
              @click.stop="to(index - 1)"
            >
              <span />
            </button>
          </div>
        </template>
      </n-carousel>
    </Transition>
  </div>
</template>

<script setup>
import { useRouter } from "vue-router";
import { getBanner } from "@/api/home";
import { useI18n } from "vue-i18n";
import { Left, Right } from "@icon-park/vue-next";

const { t } = useI18n();
const router = useRouter();
const bannerRoot = ref(null);
let bannerResizeObserver = null;

// 轮播图高度
const bannerHeight = ref(0);
const mobileControlsHeight = 26;

// 轮播图数据
const bannerType = ref("card");
const bannerData = ref([]);
const showBannerArrow = computed(() => bannerData.value.length > 1 && bannerType.value === "card");

const getBannerControlsStyle = (total) => {
  const count = Number(total) || 0;
  const inactiveWidth = 12;
  const activeWidth = bannerType.value === "slide" ? 30 : 40;
  const gap = 4;
  const horizontalChrome = 16;
  return {
    width: `${activeWidth + Math.max(count - 1, 0) * (inactiveWidth + gap) + horizontalChrome}px`,
  };
};

// 请求轮播图数据
const getBannerData = () => {
  getBanner().then((res) => {
    console.log("轮播图数据", res);
    bannerData.value = res.banners;
  });
};

// 轮播图点击事件
const bannerJump = (type, id, url) => {
  switch (type) {
    case 1:
      // 歌曲页
      router.push(`/song?id=${id}`);
      break;
    case 10:
      // 专辑页
      router.push(`/album?id=${id}`);
      break;
    case 1000:
      // 歌单页
      router.push(`/playlist?id=${id}&page=1`);
      break;
    case 1004:
      // MV页
      router.push(`/video?id=${id}`);
      break;
    case 3000:
      // 站外链接
      const time = setTimeout(() => {
        window.open(url);
      }, 2000);
      $message.loading(t("general.message.jumpOut"), {
        closable: true,
        duration: 2000,
        onClose: () => {
          clearTimeout(time);
        },
      });
      break;
    default:
      break;
  }
};

// 获取宽度计算轮播图高度
const getBannerHeight = () => {
  const width = bannerRoot.value?.clientWidth || window.innerWidth;
  if (width > 680) {
    bannerType.value = "card";
    bannerHeight.value = Math.min(300, Math.max(170, width / 4.8));
  } else {
    bannerType.value = "slide";
    bannerHeight.value = Math.max(140, width / 3 + mobileControlsHeight);
  }
};

onMounted(() => {
  getBannerData();
  getBannerHeight();
  bannerResizeObserver = new ResizeObserver(getBannerHeight);
  if (bannerRoot.value) bannerResizeObserver.observe(bannerRoot.value);
});

onBeforeUnmount(() => {
  bannerResizeObserver?.disconnect();
  bannerResizeObserver = null;
});
</script>

<style lang="scss" scoped>
.banner-shell {
  container-type: inline-size;
  width: 100%;
  min-width: 0;
}

.banner {
  position: relative;
  // max-width: 1200px;
  // margin: 0 auto;
  .item {
    border-radius: var(--radius-md);
    img {
      margin: 0 auto;
      width: 100%;
      height: 100%;
      object-fit: cover;
      cursor: pointer;
    }
  }
}

// 左右两侧导航箭头 — 玻璃拟态，悬停显现，与全站悬浮控件统一视觉
.banner-arrow {
  position: absolute;
  top: calc(50% - 21px);
  z-index: 3;
  width: 42px;
  height: 42px;
  display: flex;
  align-items: center;
  justify-content: center;
  color: #fff;
  border: 1px solid rgb(255 255 255 / 18%);
  border-radius: var(--radius-pill);
  background: rgb(0 0 0 / 28%);
  -webkit-backdrop-filter: blur(var(--blur-md)) saturate(150%);
  backdrop-filter: blur(var(--blur-md)) saturate(150%);
  box-shadow: var(--shadow-3);
  cursor: pointer;
  opacity: 0;
  transform: scale(0.85);
  pointer-events: none;
  transition:
    opacity var(--duration-300) var(--ease-out),
    transform var(--duration-300) var(--ease-out),
    background-color var(--duration-200) var(--ease-out);

  &.banner-arrow--prev {
    left: 14px;
  }

  &.banner-arrow--next {
    right: 14px;
  }

  &:hover {
    background: rgb(0 0 0 / 42%);
  }

  &:focus-visible {
    outline: 2px solid rgb(255 255 255 / 76%);
    outline-offset: 2px;
  }
}

// 悬停轮播图 / 键盘聚焦时，箭头淡入归位（与封面卡片悬停显现播放键一致）
.banner:hover .banner-arrow,
.banner-arrow:focus-visible {
  opacity: 1;
  transform: scale(1);
  pointer-events: auto;
}

// 点击回弹
.banner:hover .banner-arrow:active,
.banner-arrow:focus-visible:active {
  transform: scale(0.92);
}

.banner-controls {
  position: absolute;
  left: 50%;
  bottom: 12px;
  z-index: 2;
  box-sizing: border-box;
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 4px;
  padding: 5px 7px;
  border: 1px solid rgb(255 255 255 / 18%);
  border-radius: var(--radius-pill);
  overflow: hidden;
  background: rgb(0 0 0 / 28%);
  -webkit-backdrop-filter: blur(var(--blur-md)) saturate(150%);
  backdrop-filter: blur(var(--blur-md)) saturate(150%);
  transform: translateX(-50%);
}

.banner-control {
  width: 12px;
  height: 16px;
  border: 0;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  padding: 0;
  background: transparent;
  cursor: pointer;
  transition: width var(--duration-300) var(--ease-out);

  span {
    width: 100%;
    height: 4px;
    border-radius: var(--radius-xs);
    background-color: rgb(255 255 255 / 40%);
    box-shadow: 0 1px 4px rgb(0 0 0 / 20%);
    transition: background-color var(--duration-300) var(--ease-in-out);
  }

  &:focus-visible {
    outline: 2px solid rgb(255 255 255 / 76%);
    outline-offset: 2px;
    border-radius: var(--radius-pill);
  }

  &:hover span {
    background-color: rgb(255 255 255 / 68%);
  }

  &.is-active {
    width: 40px;

    span {
      background: #fff;
    }
  }
}

@media (max-width: 680px) {
  .banner {
    :deep(.n-carousel__slides) {
      height: calc(100% - 26px);
    }
  }

  .banner-arrow {
    display: none;
  }

  .banner-controls {
    bottom: 3px;
    padding: 4px 6px;
  }

  .banner-control {
    width: 12px;

    &.is-active {
      width: 30px;
    }
  }
}

@media (prefers-reduced-motion: reduce) {
  .banner-arrow,
  .banner-control,
  .banner-control span {
    transition: none;
  }
}

.v-enter-active,
.v-leave-active {
  transition: opacity var(--duration-300) var(--ease-out);
}
.v-enter-from,
.v-leave-to {
  opacity: 0;
}
</style>
