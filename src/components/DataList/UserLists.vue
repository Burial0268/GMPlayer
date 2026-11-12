<template>
  <div class="userlists">
    <Transition mode="out-in">
      <n-grid
        x-gap="30"
        y-gap="34"
        cols="3 mb:4 s:5 l:6"
        responsive="screen"
        :collapsed="gridCollapsed"
        :collapsed-rows="gridCollapsedRows"
        v-if="listData[0]"
        key="data"
      >
        <n-gi class="item" v-for="item in listData" :key="item.id" @click="toUser(item.id)">
          <div class="cover">
            <n-avatar
              lazy
              round
              class="coverImg"
              :src="getAvatar(item.cover)"
              fallback-src="/images/ico/user-filling.svg"
            >
              <template #placeholder>
                <div class="cover-loading">
                  <n-spin size="small" />
                </div>
              </template>
            </n-avatar>
            <img
              class="shadow"
              aria-hidden="true"
              alt=""
              loading="lazy"
              decoding="async"
              :src="getAvatar(item.cover)"
              @error="hideBrokenShadow"
            />
            <n-icon size="40" :component="PeopleSearchOne" />
          </div>
          <n-text class="name text-hidden">{{ item.name }}</n-text>
          <n-text class="sign text-hidden" :depth="3" v-if="item.signature">
            {{ item.signature }}
          </n-text>
        </n-gi>
      </n-grid>
      <n-empty v-else-if="loading === false" key="empty" class="empty" />
      <n-grid
        v-else
        key="loading"
        class="loading"
        x-gap="20"
        y-gap="26"
        cols="3 mb:4 s:5 l:6"
        responsive="screen"
        :collapsed="gridCollapsed"
        :collapsed-rows="gridCollapsedRows"
      >
        <n-gi class="item" v-for="n in loadingNum" :key="n">
          <n-skeleton class="pic" :sharp="false" />
          <n-skeleton text style="width: 60%" />
        </n-gi>
      </n-grid>
    </Transition>
  </div>
</template>

<script setup>
import { PeopleSearchOne } from "@icon-park/vue-next";
import { useRouter } from "vue-router";

const router = useRouter();
defineProps({
  // 列表数据
  listData: {
    type: Array,
    default: () => [],
  },
  // 折叠栅格
  gridCollapsed: {
    type: Boolean,
    default: false,
  },
  // 折叠后行数
  gridCollapsedRows: {
    type: Number,
    default: 1,
  },
  // 加载占位数量
  loadingNum: {
    type: Number,
    default: 12,
  },
  // 加载状态（null=旧行为，false=加载完成可显示空状态）
  loading: {
    type: Boolean,
    default: null,
  },
});

// 头像地址
const getAvatar = (url) => {
  if (!url) return "/images/ico/user-filling.svg";
  return url.replace(/^http:/, "https:") + "?param=200y200";
};

// 悬停光晕是装饰层，用裸 <img> 而不是第二个 n-avatar：同一 URL 命中缓存，
// 省下的是每格多一份组件实例与 DOM。
const hideBrokenShadow = (e) => {
  if (e.target instanceof HTMLElement) e.target.style.display = "none";
};

// 前往用户主页
const toUser = (id) => {
  if (!id) return;
  router.push({ path: "/profile", query: { id } });
};
</script>

<style lang="scss" scoped>
.userlists {
  padding-top: 20px;
  .v-enter-active,
  .v-leave-active {
    transition: opacity var(--duration-300) var(--ease-out);
  }

  .v-enter-from,
  .v-leave-to {
    opacity: 0;
  }
  .item {
    text-align: center;
    cursor: pointer;
    .cover {
      position: relative;
      display: flex;
      align-items: center;
      justify-content: center;
      box-shadow: 0 4px 16px 0 #00000020;
      border-radius: 50%;
      transition: all var(--duration-300) var(--ease-out);
      .coverImg {
        filter: brightness(1);
        transform: scale(1);
        width: 100%;
        height: 100%;
        transition: all var(--duration-300) var(--ease-out);
        z-index: 1;
        .cover-loading {
          position: relative;
          display: flex;
          align-items: center;
          justify-content: center;
          width: 100%;
          height: 0;
          padding-bottom: 100%;
          background-color: #0001;
          .n-spin-body {
            position: absolute;
            top: 0;
            height: 100%;
            display: flex;
            align-items: center;
            justify-content: center;
          }
        }
      }
      .shadow {
        opacity: 0;
        position: absolute;
        top: 12px;
        height: 100%;
        width: 100%;
        filter: blur(16px) opacity(0.6);
        transform: scale(0.92, 0.96);
        z-index: 0;
        object-fit: cover;
        border-radius: 50%;
        aspect-ratio: 1/1;
        transition: opacity var(--duration-300) var(--ease-out);
      }
      .n-icon {
        opacity: 0;
        transform: scale(0.8);
        position: absolute;
        color: #fff;
        transition: all var(--duration-300) var(--ease-out);
        z-index: 1;
      }
      @media (hover: hover) {
        &:hover {
          box-shadow: 0 4px 16px 0 #00000040;
          .n-icon {
            opacity: 1;
            transform: scale(1);
          }
          .coverImg {
            filter: brightness(0.8);
            transform: scale(1.05);
          }
          .shadow {
            opacity: 1;
          }
        }
      }
      &:active {
        .n-avatar {
          transform: scale(1);
        }
      }
    }
    .name {
      display: block;
      margin-top: 14px;
      font-size: 16px;
      transition: all var(--duration-300) var(--ease-out);
      cursor: pointer;
      @media (hover: hover) {
        &:hover {
          color: var(--main-color);
        }
      }
    }
    .sign {
      display: block;
      margin-top: 4px;
      font-size: 12px;
    }
  }
  .loading {
    .pic {
      padding-bottom: 100%;
      width: 100%;
      height: 0;
      border-radius: 50% !important;
      margin-bottom: 20px;
    }
  }
  .empty {
    margin: 40px 0;
  }
}
</style>
