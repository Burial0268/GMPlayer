<template>
  <Transition mode="out-in" :css="animated">
    <n-card v-if="Object.keys(commentData).length" class="comment" hoverable>
      <div class="user">
        <div v-button class="avatar" @click="goToUser(commentData.user.userId)">
          <img
            class="avatarImg"
            :src="commentData.user.avatarUrl.replace(/^http:/, 'https:') + '?param=50y50'"
            alt="avatar"
          />
          <img
            class="musicPackage"
            v-if="commentData.user.vipRights?.redVipAnnualCount > 0"
            :src="commentData.user.vipRights.musicPackage.iconUrl.replace(/^http:/, 'https:')"
            alt="redVipAnnualCount"
            title="网易音乐人"
          />
        </div>
        <div class="associator" v-if="commentData.user.vipRights?.redVipLevel > 0">
          <img
            v-if="commentData.user.vipRights.associator"
            :src="commentData.user.vipRights.associator.iconUrl.replace(/^http:/, 'https:')"
            alt="associator"
            title="黑胶会员"
          />
        </div>
      </div>
      <div class="review">
        <div class="content">
          <n-text class="name" @click="goToUser(commentData.user.userId)">
            {{ commentData.user.nickname }}：
          </n-text>
          <n-text class="text" v-html="commentData.content" />
        </div>
        <div class="beReplied" v-if="commentData.beReplied?.[0]">
          <n-text class="name" @click="goToUser(commentData.beReplied[0].user.userId)">
            @{{ commentData.beReplied[0].user.nickname }}：
          </n-text>
          <n-text class="text">{{ commentData.beReplied[0].content }}</n-text>
        </div>
        <div class="thing">
          <div class="item">
            <n-icon size="14" :depth="3" :component="Time" />
            <n-text :depth="3" v-html="getCommentTime(commentData.time)" />
          </div>
          <div class="item" v-if="commentData.ipLocation?.location">
            <n-icon size="14" :depth="3" :component="Local" />
            <n-text :depth="3" v-html="commentData.ipLocation.location" />
          </div>
          <div
            :class="commentData.liked ? 'like liked' : 'like'"
            role="button"
            tabindex="0"
            :aria-pressed="commentData.liked"
            @click="toLikeComment"
            @keydown.enter.prevent="toLikeComment"
            @keydown.space.prevent="toLikeComment"
          >
            <n-icon>
              <ThumbsUp :theme="commentData.liked ? 'filled' : 'outline'" />
            </n-icon>
            {{ formatNumber(commentData.likedCount) }}
          </div>
        </div>
      </div>
    </n-card>
    <n-skeleton v-else class="skeleton" />
  </Transition>
</template>

<script setup>
import { vButton } from "@/utils/buttonRole";
import { getCommentTime, formatNumber } from "@/utils/timeTools";
import { Local, Time, ThumbsUp } from "@icon-park/vue-next";
import { userStore } from "@/store";
import { useRouter } from "vue-router";
import { likeComment } from "@/api/comment";
import { useI18n } from "vue-i18n";

const { t } = useI18n();
const user = userStore();
const router = useRouter();
const props = defineProps({
  // 评论 数据
  commentData: {
    type: Object,
    default: () => ({}),
  },
  // 评论所属资源；播放器内嵌评论不依赖当前路由参数
  resourceId: {
    type: [Number, String],
    default: null,
  },
  // 嵌入虚拟列表时关闭逐条淡入，避免大量评论同时触发过渡。
  animated: {
    type: Boolean,
    default: true,
  },
});

// 前往用户主页
const goToUser = (userId) => {
  if (!userId) return;
  router.push({ path: "/profile", query: { id: userId } });
};

// 点赞评论
const toLikeComment = () => {
  if (user.userLogin) {
    const resourceId = Number(props.resourceId ?? router.currentRoute.value.query.id);
    if (!Number.isFinite(resourceId)) return;
    const type = props.commentData.liked ? 0 : 1;
    likeComment(resourceId, props.commentData.commentId, type).then((res) => {
      if (res.code === 200) {
        props.commentData.liked = !props.commentData.liked;
        props.commentData.likedCount += type ? 1 : -1;
      } else {
        $message.error(t("general.message.operationFailed"));
      }
    });
  } else {
    $message.error(t("general.message.needLogin"));
  }
};
</script>

<style lang="scss" scoped>
.v-enter-active,
.v-leave-active {
  transition: opacity var(--duration-300) var(--ease-out);
}

.v-enter-from,
.v-leave-to {
  opacity: 0;
}
.comment {
  margin-bottom: 12px;
  border-radius: var(--radius-md);
  :deep(.n-card__content) {
    display: flex;
    flex-direction: row;
    .user {
      min-width: 60px;
      width: 60px;
      display: flex;
      flex-direction: column;
      align-items: center;
      margin-right: 16px;
      .avatar {
        position: relative;
        width: 54px;
        height: 54px;
        border-radius: 50%;
        box-shadow: 0 6px 8px -2px rgb(0 0 0 / 16%);
        cursor: pointer;
        transition: transform var(--duration-150) var(--ease-out);
        &:active {
          transform: scale(0.94);
        }
        .avatarImg {
          border-radius: 50%;
          width: 100%;
          height: 100%;
        }
        .musicPackage {
          height: 16px;
          position: absolute;
          right: 0;
          bottom: 0;
          background-color: #fff;
          border-radius: 50%;
        }
      }
      .associator {
        display: flex;
        flex-direction: column;
        margin-top: 12px;
        justify-content: space-evenly;
        align-items: center;
        img {
          height: 16px;
          &:nth-of-type(1) {
            margin-bottom: 6px;
          }
        }
      }
      @media (max-width: 578px) {
        min-width: 48px;
        width: 48px;
        .avatar {
          width: 40px;
          height: 40px;
          .musicPackage {
            height: 12px;
          }
        }
      }
    }
    .review {
      width: 100%;
      display: flex;
      flex-direction: column;
      justify-content: space-between;
      .content {
        .name {
          font-weight: bold;
          cursor: pointer;
          transition: all var(--duration-300) var(--ease-out);
          &:hover {
            color: var(--main-color);
          }
        }
      }
      .beReplied {
        width: 100%;
        padding: 4px 8px;
        border-radius: var(--radius-md);
        background-color: var(--n-border-color);
        font-size: 13px;
        margin-top: 6px;
        box-sizing: border-box;
        .name {
          font-weight: bold;
          cursor: pointer;
          transition: all var(--duration-300) var(--ease-out);
          &:hover {
            color: var(--main-color);
          }
        }
      }
      .thing {
        margin-top: 12px;
        display: flex;
        align-items: center;
        .item {
          margin-right: 12px;
          font-size: 13px;
          .n-icon {
            margin-right: 4px;
          }
          .n-text {
            transform: translateY(0.5px);
            display: inline-block;
          }
        }
        .like {
          margin-left: auto;
          cursor: pointer;
          transition: all var(--duration-300) var(--ease-out);
          opacity: 0.6;
          &:hover {
            color: var(--main-color);
            opacity: 1;
          }
          &:active {
            transform: scale(0.95);
          }
          &.liked {
            color: var(--main-color);
            opacity: 1;
          }
        }
        .n-icon {
          font-size: 16px;
          transform: translateY(3px);
        }
      }
    }
  }
}
.skeleton {
  width: 100%;
  height: 120px;
  border-radius: var(--radius-md);
  margin-bottom: 12px;
}
</style>
