<template>
  <!-- 喜欢的音乐 -->
  <div class="like-song" @click="toLikeSongs">
    <div class="like-song-bg" :style="`background-image: url(${cardImage})`" />
    <div class="gray" />
    <div class="left">
      <n-icon class="icon" :component="CollectionRecords" size="30" />
      <div class="title">
        <n-text class="name">{{ $t("home.modules.likeSong.title") }}</n-text>
        <n-text class="tip">{{ $t("home.modules.likeSong.subtitle") }}</n-text>
      </div>
    </div>
    <div class="right">
      <n-icon class="icon" :component="Right" size="20" />
    </div>
  </div>
</template>

<script setup>
import { useRouter } from "vue-router";
import { userStore } from "@/store";
import { CollectionRecords, Right } from "@icon-park/vue-next";

const router = useRouter();
const user = userStore();

// 卡片背景
const cardImage = ref(null);

// 生成卡片背景
const getCardImage = (index) => {
  if (user.userLogin && user.getUserPlayLists.own[0]) {
    const num = index ?? Math.floor(Math.random() * user.getUserPlayLists.own.length);
    cardImage.value =
      user.getUserPlayLists.own[num]?.cover.replace(/^http:/, "https:") + "?param=100y100";
  } else {
    cardImage.value = "/images/pic/pic.jpg";
  }
};

// 跳转喜欢的音乐
const toLikeSongs = () => {
  if (user.userLogin) {
    const id = user.getUserPlayLists.own[0]?.id;
    if (id) {
      router.push(`/playlist?id=${id}&page=1`);
    } else {
      console.error("发生错误");
    }
  } else {
    $message.error("请登录账号后使用");
    router.push("/login");
  }
};

onMounted(() => {
  getCardImage();
  if (user.userLogin && !user.getUserPlayLists.has && !user.getUserPlayLists.isLoading) {
    user.setUserPlayLists(() => {
      getCardImage();
    });
  }
});
</script>

<style lang="scss" scoped>
.like-song {
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
  transform: translateZ(0);
  perspective: 1px;
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
  .like-song-bg {
    position: absolute;
    // 放大并超出容器，模糊后的透明边缘被 overflow:hidden 裁掉
    inset: -48px;
    background-repeat: no-repeat;
    background-size: cover;
    background-position: center;
    // 直接模糊背景图本身，避免依赖 backdrop-filter（在 translateZ/perspective 等 3D 上下文中会失效）
    filter: blur(20px);
    z-index: 0;
  }
  .gray {
    position: absolute;
    inset: 0;
    background-color: rgba(0, 0, 0, 0.4);
    pointer-events: none;
    z-index: 1;
  }
  .left {
    position: relative;
    z-index: 2;
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
    z-index: 2;
    display: flex;
    .icon {
      opacity: 0;
      transform: translateX(-8px);
      transition: all 0.3s;
    }
  }
}
</style>
