<template>
  <!-- 私人雷达 -->
  <div class="radar" @click="router.push(`/playlist?id=${radarId}&page=1`)">
    <div class="radar-bg" />
    <div class="gray" />
    <div class="left">
      <n-icon class="icon" :component="RadarThree" size="30" />
      <div class="title">
        <n-text class="name">{{ $t("home.modules.radar.title") }}</n-text>
        <n-text class="tip">{{ $t("home.modules.radar.subtitle") }}</n-text>
      </div>
    </div>
    <div class="right">
      <n-icon class="icon" :component="Right" size="20" />
    </div>
  </div>
</template>

<script setup>
import { useRouter } from "vue-router";
import { RadarThree, Right } from "@icon-park/vue-next";

const router = useRouter();

// 私人雷达歌单
const radarId = ref(3136952023);
</script>

<style lang="scss" scoped>
.radar {
  position: relative;
  color: #fff;
  height: 100%;
  width: 100%;
  display: flex;
  flex-direction: row;
  align-items: center;
  justify-content: space-between;
  border-radius: 8px;
  padding: 0 18px;
  box-sizing: border-box;
  cursor: pointer;
  z-index: 0;
  overflow: hidden;
  &:hover {
    .left {
      .title {
        .name {
          opacity: 0;
          transform: translateY(-50px);
        }
        .tip {
          opacity: 1;
          transform: translateY(0);
        }
      }
    }
    .right {
      .icon {
        opacity: 1;
        transform: translateX(0);
      }
    }
  }
  .radar-bg {
    position: absolute;
    // 放大并超出容器，模糊后的透明边缘被 overflow:hidden 裁掉
    inset: -48px;
    background-image: url("/images/pic/radar.jpg");
    background-repeat: no-repeat;
    background-size: cover;
    background-position: center;
    // 直接模糊背景图本身，避免依赖 backdrop-filter（在部分 WebView2/3D 上下文中会失效）
    filter: blur(20px);
    z-index: 0;
  }
  .gray {
    position: absolute;
    inset: 0;
    background-color: #00000010;
    pointer-events: none;
    z-index: 0;
  }
  .left {
    position: relative;
    z-index: 1;
    height: 100%;
    width: 100%;
    display: flex;
    align-items: center;
    .icon {
      margin-right: 12px;
    }
    .title {
      height: 100%;
      width: 100%;
      position: relative;
      display: flex;
      flex-direction: column;
      justify-content: center;
      .name {
        color: #fff;
        font-size: 18px;
        transition: all 0.3s;
        @media (max-width: 1020px) {
          font-size: 16px;
        }
      }
      .tip {
        height: 100%;
        display: flex;
        align-items: center;
        position: absolute;
        color: #fff;
        opacity: 0;
        transform: translateY(50px);
        transition: all 0.3s;
      }
    }
  }
  .right {
    position: relative;
    z-index: 1;
    display: flex;
    .icon {
      opacity: 0;
      transform: translateX(-8px);
      transition: all 0.3s;
    }
  }
}
</style>
