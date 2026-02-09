<template>
  <Teleport to="body">
    <div ref="bigPlayerRef"
    :class="[
      'bplayer',
      `bplayer-${setting.backgroundImageShow}`,
      isMobile ? 'mobile-player' : 'desktop-player',
      music.showBigPlayer ? 'opened' : '',
      isMobile && mobileLayer === 2 ? 'layer2-active' : ''
    ]"
    :style="{
      '--cover-bg': songPicGradient,
      '--main-cover-color': `rgb(${setting.immersivePlayer ? songPicColor : '255,255,255'})`
    }">
      <!-- 共用部分: 背景和顶部菜单 -->
      <Transition name="fade" mode="out-in">
        <div :key="`bg--${songPicGradient}`" :class="['overlay', setting.backgroundImageShow]">
          <template v-if="setting.backgroundImageShow === 'blur'">
            <BlurBackgroundRender v-if="music.getPlaySongData" :fps="music.getPlayState ? setting.fps || 30 : 0"
              :playing="actualPlayingProp" :album="coverImageUrl" :blurLevel="setting.blurAmount || 30"
              :saturation="setting.contrastAmount || 1.2" :renderScale="setting.renderScale || 0.5"
              class="blur-webgl" />
          </template>
        </div>
      </Transition>

      <template v-if="setting.backgroundImageShow == 'eplor'">
        <BackgroundRender :fps="music.getPlayState ? setting.fps : 0" :playing="actualPlayingProp"
          :flowSpeed="music.getPlayState ? setting.flowSpeed : 0"
          :album="setting.albumImageUrl === 'none' ? coverImageUrl : setting.albumImageUrl"
          :renderScale="setting.renderScale" :lowFreqVolume="computedLowFreqVolume" :staticMode="!music.showBigPlayer"
          class="overlay" />
      </template>

      <div :class="setting.backgroundImageShow === 'blur' ? 'gray blur' : 'gray'" />

      <div class="icon-menu">
        <div class="menu-left">
          <div v-if="setting.showLyricSetting" class="icon">
            <n-icon class="setting" size="30" :component="SettingsRound" @click="LyricSettingRef.openLyricSetting()" />
          </div>
        </div>
        <div class="menu-right">
          <div class="icon">
            <n-icon class="screenfull" :component="screenfullIcon" @click="screenfullChange" />
          </div>
          <div class="icon">
            <n-icon class="close" :component="KeyboardArrowDownFilled" @click="closeBigPlayer" />
          </div>
        </div>
      </div>

      <!-- 移动端布局 — 照搬 AMLL verticalLayout 结构 -->
      <template v-if="isMobile">
        <!-- AMLL .thumb — 抽屉把手 -->
        <div class="mobile-thumb" @click="closeBigPlayer">
          <div class="handle-bar"></div>
        </div>

        <!-- AMLL .lyricLayout — Layer 2: 紧凑封面信息 + 歌词 -->
        <div class="mobile-lyric-layout">
          <div class="mobile-phony-small-cover" ref="phonySmallCoverRef" />
          <div class="mobile-small-controls">
            <div class="mobile-song-info">
              <div class="name-wrapper">
                <div class="name" :class="{ 'is-scrolling': isNameOverflow }">
                  <span class="name-inner">{{ songName || $t("other.noSong") }}</span>
                  <span class="name-inner" v-if="isNameOverflow">{{ songName || $t("other.noSong") }}</span>
                </div>
              </div>
              <div class="artists text-hidden" v-if="artistList.length">
                <span v-for="(item, index) in artistList" :key="'s' + index">
                  {{ item.name }}<span v-if="index != artistList.length - 1"> / </span>
                </span>
              </div>
            </div>
            <div class="mobile-header-actions">
              <n-icon size="24"
                :component="music.getPlaySongData && music.getSongIsLike(music.getPlaySongData.id) ? StarRound : StarBorderRound"
                @click.stop="
                  music.getPlaySongData && (
                    music.getSongIsLike(music.getPlaySongData.id)
                      ? music.changeLikeList(music.getPlaySongData.id, false)
                      : music.changeLikeList(music.getPlaySongData.id, true)
                  )
                  " />
              <n-icon size="24" :component="MoreVertRound" @click.stop="" />
            </div>
          </div>
          <div class="mobile-lyric" v-if="music.getPlaySongLyric && music.getPlaySongLyric.lrc &&
            music.getPlaySongLyric.lrc[0] &&
            music.getPlaySongLyric.lrc.length > 4">
            <RollingLyrics @mouseenter="lrcMouseStatus = setting.lrcMousePause ? true : false" @mouseleave="lrcAllLeave"
              @lrcTextClick="lrcTextClick" class="mobile-lyrics"></RollingLyrics>
          </div>
          <div v-else class="no-lyrics"><span>¯\_(ツ)_/¯</span></div>
        </div>

        <!-- AMLL .noLyricLayout — Layer 1: 大封面 + 歌曲信息 + controls -->
        <div class="mobile-cover-layout">
          <div class="mobile-phony-big-cover" ref="phonyBigCoverRef" />
          <div class="mobile-big-controls">
            <!-- 歌曲信息（展开） -->
            <div class="mobile-song-info-row">
              <div class="mobile-song-info">
                <div class="name-wrapper" ref="nameWrapperRef">
                  <div class="name" ref="nameTextRef" :class="{ 'is-scrolling': isNameOverflow }">
                    <span class="name-inner">{{ songName || $t("other.noSong") }}</span>
                    <span class="name-inner" v-if="isNameOverflow">{{ songName || $t("other.noSong") }}</span>
                  </div>
                </div>
                <div class="artists text-hidden" v-if="artistList.length">
                  <span v-for="(item, index) in artistList" :key="'b' + index">
                    {{ item.name }}<span v-if="index != artistList.length - 1"> / </span>
                  </span>
                </div>
              </div>
              <div class="mobile-header-actions">
                <n-icon size="24"
                  :component="music.getPlaySongData && music.getSongIsLike(music.getPlaySongData.id) ? StarRound : StarBorderRound"
                  @click.stop="
                    music.getPlaySongData && (
                      music.getSongIsLike(music.getPlaySongData.id)
                        ? music.changeLikeList(music.getPlaySongData.id, false)
                        : music.changeLikeList(music.getPlaySongData.id, true)
                    )
                    " />
                <n-icon size="24" :component="MoreVertRound" @click.stop="" />
              </div>
            </div>
            <!-- 进度条 -->
            <div class="mobile-progress">
              <BouncingSlider :value="music.getPlaySongTime.currentTime || 0" :min="0"
                :max="music.getPlaySongTime.duration || 1" :is-playing="music.getPlayState"
                @update:value="handleProgressSeek" @seek-start="music.setPlayState(false)"
                @seek-end="music.setPlayState(true)" />
              <div class="time-display">
                <span>{{ music.getPlaySongTime.songTimePlayed }}</span>
                <span>-{{ remainingTime }}</span>
              </div>
            </div>
            <!-- 控制按钮 -->
            <div class="mobile-control-buttons">
              <n-icon class="mode-btn" size="22"
                :component="persistData.playSongMode === 'random' ? ShuffleOne : persistData.playSongMode === 'single' ? PlayOnce : PlayCycle"
                @click.stop="music.setPlaySongMode()" />
              <n-icon v-if="!music.getPersonalFmMode" class="prev" size="36" :component="IconRewind"
                @click.stop="music.setPlaySongIndex('prev')" />
              <n-icon v-else class="dislike" size="28" :component="ThumbDownRound"
                @click="music.setFmDislike(music.getPersonalFmData.id)" />
              <div class="play-state">
                <n-button :loading="music.getLoadingState" secondary circle :keyboard="false" :focusable="false"
                  @click.stop="music.setPlayState(!music.getPlayState)">
                  <template #icon>
                    <Transition name="fade" mode="out-in">
                      <n-icon size="64" :component="music.getPlayState ? IconPause : IconPlay" />
                    </Transition>
                  </template>
                </n-button>
              </div>
              <n-icon class="next" size="36" :component="IconForward" @click.stop="music.setPlaySongIndex('next')" />
              <n-icon class="mode-btn" size="22" :component="MessageRound" @click.stop="toComment" />
            </div>
            <!-- 音量控制 -->
            <div class="mobile-volume">
              <BouncingSlider :value="persistData.playVolume" :min="0" :max="1" :change-on-drag="true"
                @update:value="val => persistData.playVolume = val">
                <template #before-icon>
                  <n-icon size="20" :component="VolumeOffRound" />
                </template>
                <template #after-icon>
                  <n-icon size="20" :component="VolumeUpRound" />
                </template>
              </BouncingSlider>
            </div>
          </div>
        </div>

        <!-- AMLL .coverFrame — 唯一 spring 动画元素 -->
        <div v-if="currentCoverStyle" class="mobile-cover-frame" :style="coverFrameStyle"
          @click="switchMobileLayer(mobileLayer === 1 ? 2 : 1)">
          <img :src="coverImageUrl500" alt="cover" />
        </div>
      </template>

      <!-- 桌面端布局 -->
      <template v-else>
        <div :class="[
          music.getPlaySongLyric && music.getPlaySongLyric.lrc && music.getPlaySongLyric.lrc[0] && music.getPlaySongLyric.lrc.length > 4 && !music.getLoadingState
            ? 'all'
            : 'all noLrc'
        ]">
          <div class="tip" ref="tipRef" v-show="lrcMouseStatus">
            <n-text>{{ $t("other.lrcClicks") }}</n-text>
          </div>

          <div class="left" ref="leftContentRef">
            <PlayerCover v-if="setting.playerStyle === 'cover'" />
            <PlayerRecord v-else-if="setting.playerStyle === 'record'" />
          </div>

          <div class="right" ref="rightContentRef" v-if="
            music.getPlaySongLyric && music.getPlaySongLyric.lrc &&
            music.getPlaySongLyric.lrc[0] &&
            music.getPlaySongLyric.lrc.length > 4 &&
            !music.getLoadingState
          ">
            <div class="lrcShow">
              <RollingLyrics @mouseenter="lrcMouseStatus = setting.lrcMousePause ? true : false"
                @mouseleave="lrcAllLeave" @lrcTextClick="lrcTextClick"></RollingLyrics>

              <div
                :class="menuShow ? 'menu show' : 'menu'"
                v-show="setting.playerStyle === 'record'">
                <div class="time">
                  <span>{{ music.getPlaySongTime.songTimePlayed }}</span>
                  <BouncingSlider :value="music.getPlaySongTime.currentTime || 0" :min="0"
                    :max="music.getPlaySongTime.duration || 1" :is-playing="music.getPlayState"
                    @update:value="handleProgressSeek" @seek-start="music.setPlayState(false)"
                    @seek-end="music.setPlayState(true)" />
                  <span>{{ music.getPlaySongTime.songTimeDuration }}</span>
                </div>
                <div class="control">
                  <n-icon v-if="!music.getPersonalFmMode" class="prev" size="30" :component="IconRewind"
                    @click.stop="music.setPlaySongIndex('prev')" />
                  <n-icon v-else class="dislike" :component="ThumbDownRound"
                    @click="music.setFmDislike(music.getPersonalFmData.id)" />
                  <div class="play-state">
                    <n-button :loading="music.getLoadingState" secondary circle :keyboard="false" :focusable="false">
                      <template #icon>
                        <Transition name="fade" mode="out-in">
                          <n-icon size="42" :component="music.getPlayState ? IconPause : IconPlay"
                            @click.stop="music.setPlayState(!music.getPlayState)" />
                        </Transition>
                      </template>
                    </n-button>
                  </div>
                  <n-icon class="next" size="30" :component="IconForward"
                    @click.stop="music.setPlaySongIndex('next')" />
                </div>
              </div>
            </div>
          </div>
        </div>
      </template>

      <!-- 共用组件 -->
      <Spectrum v-if="setting.musicFrequency" :height="60" :show="music.showBigPlayer" />
      <LyricSetting ref="LyricSettingRef" />
    </div>
  </Teleport>
</template>

<script setup lang="ts">
import {
  KeyboardArrowDownFilled,
  FullscreenRound,
  FullscreenExitRound,
  SettingsRound,
  ThumbDownRound,
  StarBorderRound,
  StarRound,
  MoreVertRound,
  MessageRound,
  VolumeUpRound,
  VolumeOffRound,
  VolumeMuteRound,
} from "@vicons/material";
import { ShuffleOne, PlayOnce, PlayCycle } from "@icon-park/vue-next";
import { Motion } from "motion-v";
import { musicStore, settingStore, siteStore } from "@/store";
import { useRouter } from "vue-router";
import { setSeek } from "@/utils/AudioContext";
import PlayerRecord from "./PlayerRecord.vue";
import PlayerCover from "./PlayerCover.vue";
import RollingLyrics from "./RollingLyrics.vue";
import Spectrum from "./Spectrum.vue";
import LyricSetting from "@/components/DataModal/LyricSetting.vue";
import screenfull from "screenfull";
import BouncingSlider from "./BouncingSlider.vue";
import BackgroundRender from "@/libs/apple-music-like/BackgroundRender.vue";
import { storeToRefs } from "pinia";
import gsap from "gsap";
import {
  onMounted,
  nextTick,
  watch,
  ref,
  shallowRef,
  computed,
  onBeforeUnmount
} from "vue";
import BlurBackgroundRender from "./BlurBackgroundRender.vue";

// 导入 svg 图标
import IconPlay from "./icons/IconPlay.vue";
import IconPause from "./icons/IconPause.vue";
import IconForward from "./icons/IconForward.vue";
import IconRewind from "./icons/IconRewind.vue";
import "./icons/icon-animations.css";

const router = useRouter();
const music = musicStore();
const site = siteStore();
const setting = settingStore();

const { songPicGradient, songPicColor } = storeToRefs(site)
const { persistData } = storeToRefs(music)

// AMLL pattern: spring 物理动画配置 (stiffness: 200, damping: 30)


// 创建需要的refs用于GSAP动画
const bigPlayerRef = ref(null);
const tipRef = ref(null);
const leftContentRef = ref(null);
const rightContentRef = ref(null);

// 检测是否为移动设备
const isMobile = ref(false);

// 移动端当前显示层级 (1=控制层, 2=歌词层)
const mobileLayer = ref(1);

// 移动端元素引用
const nameWrapperRef = ref(null);
const nameTextRef = ref(null);

// AMLL pattern: phony 占位元素引用
const phonyBigCoverRef = ref(null);
const phonySmallCoverRef = ref(null);

// 封面动画状态
const currentCoverStyle = ref(null);

// 歌曲名称是否溢出（需要滚动）
const isNameOverflow = ref(false);

// AMLL calcCoverLayout: 从 phony 元素测量封面位置 (getBoundingClientRect)
const calcCoverLayout = (hideLyric = true) => {
  const root = bigPlayerRef.value;
  if (!root) return undefined;
  const targetCover = hideLyric
    ? phonyBigCoverRef.value
    : phonySmallCoverRef.value;
  if (!targetCover) return undefined;
  let rootEl = root;
  // AMLL pattern: 跳过 display: contents 的元素
  while (getComputedStyle(rootEl).display === 'contents') {
    rootEl = rootEl.parentElement;
  }
  const rootB = rootEl.getBoundingClientRect();
  const targetB = targetCover.getBoundingClientRect();
  const size = Math.min(targetCover.clientWidth, targetCover.clientHeight);
  if (size <= 0) return undefined;
  const result = {
    width: size,
    height: size,
    left: targetB.x - rootB.x + (targetB.width - size) / 2,
    top: targetB.y - rootB.y + (targetB.height - size) / 2,
    borderRadius: hideLyric ? 12 : 8,
  };
  console.log('[calcCoverLayout]', hideLyric ? 'big' : 'small', result, 'phony size:', targetCover.clientWidth, targetCover.clientHeight);
  return result;
};

// 更新封面动画目标
const updateCoverStyle = () => {
  const hideLyric = mobileLayer.value === 1;
  currentCoverStyle.value = calcCoverLayout(hideLyric);
};

// 封面内联样式（直接应用，无动画调试用 → 后续替换为 spring 动画）
const coverFrameStyle = computed(() => {
  const s = currentCoverStyle.value;
  if (!s) return {};
  return {
    width: s.width + 'px',
    height: s.height + 'px',
    left: s.left + 'px',
    top: s.top + 'px',
    borderRadius: s.borderRadius + 'px',
  };
});

// ResizeObserver
let layoutResizeObserver = null;

// 计算剩余时间 (负数 ETA)
const remainingTime = computed(() => {
  const playTime = music.getPlaySongTime;
  if (!playTime || !playTime.duration) return '0:00';

  const remaining = Math.max(0, playTime.duration - (playTime.currentTime || 0));
  const minutes = Math.floor(remaining / 60);
  const seconds = Math.floor(remaining % 60);
  return `${minutes}:${seconds.toString().padStart(2, '0')}`;
});

// 计算 lowFreqVolume 的最终值
const computedLowFreqVolume = computed(() => {
  if (!setting.dynamicFlowSpeed) return 1.0;
  // Round to 2 decimal places to avoid excessive reactivity triggering
  return Math.round(music.lowFreqVolume * 100) / 100;
});

// 缓存封面图片 URL，避免模板中多次计算 replace
const coverImageUrl = computed(() => {
  if (!music.getPlaySongData?.album?.picUrl) return '/images/pic/default.png';
  return music.getPlaySongData.album.picUrl.replace(/^http:/, 'https:');
});

// 缓存 500x500 封面（移动端用）
const coverImageUrl500 = computed(() => coverImageUrl.value + '?param=500y500');

// 缓存歌手列表
const artistList = computed(() => music.getPlaySongData?.artist ?? []);

// 缓存歌曲名
const songName = computed(() => music.getPlaySongData?.name ?? '');

// 检测歌曲名称是否溢出
const checkNameOverflow = () => {
  if (!isMobile.value) return;

  nextTick(() => {
    const wrapper = nameWrapperRef.value;
    const text = nameTextRef.value;
    if (wrapper && text) {
      const inner = text.querySelector('.name-inner');
      if (inner) {
        isNameOverflow.value = inner.scrollWidth > wrapper.clientWidth;
      }
    }
  });
};

// 初始化移动端元素状态
const initMobileElements = () => {
  if (!isMobile.value) return;

  nextTick(() => {
    // 检测名称是否溢出
    checkNameOverflow();
  });
};

// 切换移动端层级
const switchMobileLayer = (targetLayer: number) => {
  if (targetLayer === mobileLayer.value) return;
  mobileLayer.value = targetLayer;
  updateCoverStyle();
};

// 检测是否页面上已有标题组件
const hasHeaderComponent = ref(false);

// 检测视窗尺寸变化，更新移动设备状态
const updateDeviceStatus = () => {
  isMobile.value = window.innerWidth <= 768;
  // 视窗尺寸变化时也更新封面位置
  nextTick(() => updateCoverStyle());
};

// 检测页面上是否存在标题组件
const checkHeaderComponent = () => {
  // 检查页面上是否已经存在歌曲标题组件
  const headerElements = document.querySelectorAll('.page-title, .song-title, .header-title');
  hasHeaderComponent.value = headerElements.length > 0;
};

// State to force playing=true for the initial tick only
const forcePlaying = ref(true);

// Computed property to determine the actual value for the :playing prop
const actualPlayingProp = computed(() => {
  const isForced = forcePlaying.value;
  const realState = music.getPlayState;
  // Calculate the result based on the logic: force true initially, then follow real state
  const result = isForced || realState;
  // Log the dependencies and the result for debugging
  console.log(
    `-- computed actualPlayingProp -- forcePlaying: ${isForced}, music.getPlayState: ${realState}, result: ${result}`
  );
  return result;
});

// 工具栏显隐
const menuShow = ref(false);

// 歌词设置弹窗
const LyricSettingRef = ref(null);

// 关闭大播放器 (CSS transition handles animation)
const closeBigPlayer = () => {
  music.setBigPlayerState(false);
};

// 歌词文本点击事件
const lrcTextClick = (time: number) => {
  if (typeof window.$player !== "undefined") {
    // 防止soundStop被调用
    music.persistData.playSongTime.currentTime = time;
    window.$player.seek(time);
    music.setPlayState(true);
  }
  lrcMouseStatus.value = false;
};

// 歌曲进度条更新 (BouncingSlider - mobile)
const handleProgressSeek = (val: number) => {
  if (typeof window.$player !== "undefined" && music.getPlaySongTime?.duration) {
    music.persistData.playSongTime.currentTime = val;
    setSeek(window.$player, val);
  }
};

// 鼠标移出歌词区域
const lrcMouseStatus = ref(false);
const lrcAllLeave = () => {
  lrcMouseStatus.value = false;
  lyricsScroll(music.getPlaySongLyricIndex);
};

// 使用GSAP动画显示提示文本
const animateTip = (isVisible: boolean) => {
  if (!tipRef.value) return;

  if (isVisible) {
    gsap.fromTo(tipRef.value,
      { opacity: 0, y: -20 },
      { opacity: 1, y: 0, duration: 0.3, ease: "power2.out" }
    );
  } else {
    gsap.to(tipRef.value, {
      opacity: 0,
      y: -20,
      duration: 0.3,
      ease: "power2.in"
    });
  }
};

// 全屏切换
const timeOut = ref(null);
const screenfullIcon = shallowRef(FullscreenRound);
const screenfullChange = () => {
  if (screenfull.isEnabled) {
    screenfull.toggle();
    // 添加全屏切换动画
    gsap.fromTo(bigPlayerRef.value,
      { scale: screenfull.isFullscreen ? 1.05 : 0.95 },
      { scale: 1, duration: 0.4, ease: "elastic.out(1, 0.5)" }
    );

    screenfullIcon.value = screenfull.isFullscreen
      ? FullscreenRound
      : FullscreenExitRound;
    // 延迟一段时间执行列表滚动
    timeOut.value = setTimeout(() => {
      lrcMouseStatus.value = false;
      lyricsScroll(music.getPlaySongLyricIndex);
    }, 500);
  }
};

// 前往评论 | 暂时废弃
const toComment = () => {
  music.setBigPlayerState(false);
  router.push({
    path: "/comment",
    query: {
      id: music.getPlaySongData ? music.getPlaySongData.id : null,
    },
  });
};

// 歌词滚动
// 歌词滚动
const lyricsScroll = (index: number) => {
  const lrcType = !music.getPlaySongLyric.hasYrc || !setting.showYrc ? "lrc" : "yrc";
  const el = document.getElementById(lrcType + index);

  if (!el || lrcMouseStatus.value) return;

  // 获取歌词容器元素
  const container = el.parentElement;
  if (!container) return;

  const containerHeight = container.clientHeight;

  // 计算滚动距离
  let scrollDistance: number;
  if (isMobile.value) {
    scrollDistance = el.offsetTop - container.offsetTop - containerHeight * 0.35;
  } else {
    scrollDistance = el.offsetTop - container.offsetTop - containerHeight * 0.35;
  }

  // 执行滚动 (这里之前缺少了执行代码)
  container.scrollTo({
    top: scrollDistance,
    behavior: "smooth",
  });
};

// 改变 PWA 应用标题栏颜色
const changePwaColor = () => {
  const themeColorMeta = document.querySelector('meta[name="theme-color"]');
  if (!themeColorMeta) return;
  
  if (music.showBigPlayer) {
    themeColorMeta.setAttribute("content", songPicColor.value);
  } else {
    if (setting.getSiteTheme === "light") {
      themeColorMeta.setAttribute("content", "#ffffff");
    } else if (setting.getSiteTheme === "dark") {
      themeColorMeta.setAttribute("content", "#18181c");
    }
  }
};

// 使用GSAP动画显示播放器内部元素
const animatePlayerIn = () => {
  if (!bigPlayerRef.value || isMobile.value) return;

  if (leftContentRef.value) {
    gsap.fromTo(leftContentRef.value,
      { opacity: 0, scale: 0.96 },
      { opacity: 1, scale: 1, duration: 0.5, delay: 0.15, ease: "power2.out" }
    );
  }

  if (rightContentRef.value) {
    gsap.fromTo(rightContentRef.value,
      { opacity: 0, scale: 0.96 },
      { opacity: 1, scale: 1, duration: 0.5, delay: 0.25, ease: "power2.out" }
    );
  }
};

onMounted(() => {
  updateDeviceStatus();
  window.addEventListener('resize', updateDeviceStatus);
  checkHeaderComponent();

  gsap.config({
    force3D: true,
    nullTargetWarn: false
  });

  initMobileElements();

  nextTick(() => {
    updateCoverStyle();
    layoutResizeObserver = new ResizeObserver(updateCoverStyle);
    if (phonyBigCoverRef.value) layoutResizeObserver.observe(phonyBigCoverRef.value);
    if (phonySmallCoverRef.value) layoutResizeObserver.observe(phonySmallCoverRef.value);
    if (bigPlayerRef.value) layoutResizeObserver.observe(bigPlayerRef.value);

    forcePlaying.value = false;
    lyricsScroll(music.getPlaySongLyricIndex);
  });
});

onBeforeUnmount(() => {
  clearTimeout(timeOut.value);
  window.removeEventListener('resize', updateDeviceStatus);
  layoutResizeObserver?.disconnect();
});

// 监听器 (移出 onMounted 到顶层)
watch(() => music.showBigPlayer, (val) => {
  changePwaColor();
  if (val) {
    checkHeaderComponent();
    initMobileElements();
    nextTick().then(() => {
      updateCoverStyle();
      music.showPlayList = false;
      lyricsScroll(music.getPlaySongLyricIndex);
      animatePlayerIn();
    });
  }
});

watch(() => isMobile.value, () => {
  nextTick(() => {
    updateCoverStyle();
    lyricsScroll(music.getPlaySongLyricIndex);
  });
});

watch(() => lrcMouseStatus.value, (val) => animateTip(val));
watch(() => music.getPlaySongLyricIndex, (val) => lyricsScroll(val));
watch(() => site.songPicColor, () => changePwaColor());
watch(() => music.getPlaySongData, () => checkNameOverflow(), { immediate: true });

</script>

<style lang="scss" scoped>
.bplayer {
  position: fixed;
  top: 0;
  left: 0;
  width: 100%;
  height: 100%;
  z-index: 2000;
  overflow: hidden;
  color: var(--main-cover-color);
  background-repeat: no-repeat;
  background-size: 150% 150%;
  background-position: center;
  display: flex;
  justify-content: center;
  background-color: #222;
  will-change: transform;

  // AMLL-style slide-up transition (closed state)
  pointer-events: none;
  transform: translateY(100%);
  border-radius: 1em 1em 0 0;
  transition:
    border-radius 0.25s ease,
    transform 0.5s cubic-bezier(0.25, 1, 0.5, 1);

  // Opened state
  &.opened {
    pointer-events: auto;
    transform: translateY(0%);
    border-radius: 0;
    transition:
      border-radius 0.25s 0.25s ease,
      transform 0.5s cubic-bezier(0.25, 1, 0.5, 1);
  }

  /* Apple Music 风格 */
  &.apple-style {
    font-family: -apple-system, BlinkMacSystemFont, 'SF Pro Display', sans-serif;

    .gray {
      background-color: rgba(0, 0, 0, 0.5);
      -webkit-backdrop-filter: saturate(180%) blur(40px);
      backdrop-filter: saturate(180%) blur(40px);
    }

    .overlay {
      &.blur .overlay-img {
        filter: blur(120px) contrast(1.2) saturate(1.5);
      }
    }

    .icon-menu {
      .icon {
        border-radius: 50%;
        background-color: rgba(255, 255, 255, 0.1);

        &:hover {
          background-color: rgba(255, 255, 255, 0.2);
        }

        .n-icon {
          color: white;
        }
      }
    }

    .all.apple-layout {
      padding: 0;

      .left {
        width: 35%;
        padding-right: 2rem;
        display: flex;
        justify-content: center;
        align-items: center;

        // 增强封面效果
        :deep(.cover-container),
        :deep(.record-container) {
          border-radius: 8px;
          overflow: hidden;
          box-shadow: 0 10px 30px rgba(0, 0, 0, 0.3);
          transform: perspective(1000px) rotateY(0deg);
          transition: transform 0.5s ease;

          &:hover {
            transform: perspective(1000px) rotateY(-10deg);
          }

          img {
            border-radius: 8px;
          }
        }
      }

      .right {
        .lrcShow {
          .data {
            margin-bottom: 24px;

            .name {
              font-size: 2rem;
              font-weight: 600;
              margin-bottom: 8px;
              letter-spacing: -0.01em;

              span:nth-of-type(2) {
                font-weight: 400;
                opacity: 0.8;
              }
            }

            .artists {
              font-size: 1.25rem;
              opacity: 0.7;
              letter-spacing: -0.01em;
            }
          }

          .apple-lyrics {
            height: 40vh;
            overflow-y: auto;
            padding: 10px 0;
            margin-bottom: 20px;

            &::-webkit-scrollbar {
              width: 6px;
            }

            &::-webkit-scrollbar-track {
              background: transparent;
            }

            &::-webkit-scrollbar-thumb {
              background-color: rgba(255, 255, 255, 0.2);
              border-radius: 6px;
            }

            &::-webkit-scrollbar-thumb:hover {
              background-color: rgba(255, 255, 255, 0.4);
            }
          }
        }
      }
    }
  }

  /* ═══════════════════════════════════════════════════════
     移动端样式 — 照搬 AMLL verticalLayout 结构
     ═══════════════════════════════════════════════════════ */
  &.mobile-player {
    // AMLL .verticalLayout
    display: grid;
    min-height: 0;
    min-width: 0;
    // override flex-era justify-content: center (会导致 auto 列居中而非撑满)
    justify-content: stretch;
    // overflow: hidden 已在 .bplayer 设置
    grid-template-rows:
      [thumb] calc(env(safe-area-inset-top, 0px) + 30px) [main-view] 1fr;
    grid-template-columns: 1fr;

    // ── AMLL .thumb ──
    .mobile-thumb {
      grid-row: thumb;
      justify-self: center;
      align-self: end;
      z-index: 1;
      mix-blend-mode: plus-lighter;
      cursor: pointer;
      width: 60px;
      height: 20px;
      display: flex;
      align-items: center;
      justify-content: center;

      .handle-bar {
        width: 36px;
        height: 5px;
        background: rgba(255, 255, 255, 0.3);
        border-radius: 3px;
        transition: background 0.2s ease;
      }

      &:active .handle-bar {
        background: rgba(255, 255, 255, 0.5);
      }
    }

    // ── AMLL .lyricLayout — Layer 2: 紧凑封面/信息 + 歌词 ──
    .mobile-lyric-layout {
      grid-row: main-view;
      grid-column: 1 / 2;
      display: grid;
      grid-template-rows: 8px [controls] 0fr [lyric-view] 1fr;
      grid-template-columns: 16px [cover-side] 0fr [info-side] 1fr 16px;
      mix-blend-mode: plus-lighter;
    }

    // ── AMLL .noLyricLayout — Layer 1: 大封面 + controls ──
    .mobile-cover-layout {
      grid-row: main-view;
      grid-column: 1 / 2;
      overflow-y: hidden;
      display: grid;
      grid-template-rows: 1em [cover-view] 1fr [controls-view] 0fr;
      grid-template-columns: 24px [main-view] 1fr 24px;
      pointer-events: none;
    }

    // ── AMLL .phonySmallCover ──
    .mobile-phony-small-cover {
      grid-row: controls;
      grid-column: cover-side;
      justify-self: center;
      align-self: center;
      aspect-ratio: 1 / 1;
      opacity: 0;
      pointer-events: none;
      width: 56px;
    }

    // ── AMLL .smallControls — 紧凑歌曲信息 ──
    .mobile-small-controls {
      grid-row: controls;
      grid-column: info-side;
      align-self: center;
      transition: opacity 0.25s 0.25s;
      padding-left: 12px;
      min-width: 0;
      overflow: visible;
      height: fit-content;
      z-index: 1;
      mix-blend-mode: plus-lighter;
      display: flex;
      align-items: center;
      justify-content: space-between;

      .mobile-song-info {
        flex: 1;
        min-width: 0;
        overflow: hidden;

        .name-wrapper {
          overflow: hidden;
          width: 100%;

          .name {
            display: flex;
            font-weight: 600;
            font-size: 0.95rem;
            color: var(--main-cover-color);
            margin-bottom: 2px;
            white-space: nowrap;

            .name-inner {
              flex-shrink: 0;
              padding-right: 3em;
            }

            &.is-scrolling {
              animation: marquee-scroll 12s linear infinite;
            }
          }
        }

        .artists {
          font-size: 0.75rem;
          opacity: 0.7;
          color: var(--main-cover-color);
        }
      }

      .mobile-header-actions {
        display: flex;
        align-items: center;
        gap: 12px;
        margin-left: 12px;
        flex-shrink: 0;

        .n-icon {
          color: var(--main-cover-color);
          opacity: 0.8;
          cursor: pointer;

          &:active {
            opacity: 0.5;
          }
        }
      }
    }

    // ── AMLL .lyric — 歌词区域 ──
    .mobile-lyric {
      grid-row: lyric-view;
      grid-column: 1 / -1;
      transition: opacity 0.5s 0.5s;
      opacity: 1;
      mix-blend-mode: plus-lighter;
      min-height: 0;
      mask-image: linear-gradient(transparent 0%, black 8%, black 100%);

      .mobile-lyrics {
        height: 100%;
        overflow-y: auto;
        padding: 0;
        -ms-overflow-style: none;
        scrollbar-width: none;

        &::-webkit-scrollbar {
          display: none;
        }
      }
    }

    .no-lyrics {
      grid-row: lyric-view;
      grid-column: 1 / -1;
      display: flex;
      align-items: center;
      justify-content: center;
      transition: opacity 0.5s 0.5s;
      opacity: 1;

      span {
        font-size: 1rem;
        color: var(--main-cover-color);
        opacity: 0.5;
      }
    }

    // ── AMLL .phonyBigCover ──
    .mobile-phony-big-cover {
      grid-row: cover-view;
      grid-column: 2 / 3;
      opacity: 0;
      pointer-events: none;
    }

    // ── AMLL .bigControls — 完整 controls ──
    .mobile-big-controls {
      grid-row: controls-view;
      grid-column: 2 / 3;
      transition: opacity 0.5s;
      opacity: 0;
      mix-blend-mode: plus-lighter;
      min-width: 0;
      z-index: 2;
      text-shadow: 0 0 0.3em color-mix(in srgb, currentColor 15%, transparent);
      padding-bottom: calc(env(safe-area-inset-bottom) + 16px);

      // 歌曲信息（展开）
      .mobile-song-info-row {
        display: flex;
        align-items: center;
        justify-content: space-between;
        margin-bottom: 16px;

        .mobile-song-info {
          flex: 1;
          min-width: 0;
          overflow: hidden;

          .name-wrapper {
            overflow: hidden;
            width: 100%;

            .name {
              display: flex;
              font-weight: 600;
              font-size: 1.2rem;
              color: var(--main-cover-color);
              margin-bottom: 4px;
              white-space: nowrap;

              .name-inner {
                flex-shrink: 0;
                padding-right: 3em;
              }

              &.is-scrolling {
                animation: marquee-scroll 12s linear infinite;
              }
            }
          }

          .artists {
            font-size: 0.9rem;
            opacity: 0.7;
            color: var(--main-cover-color);
          }
        }

        .mobile-header-actions {
          display: flex;
          align-items: center;
          gap: 16px;
          margin-left: 12px;
          flex-shrink: 0;

          .n-icon {
            color: var(--main-cover-color);
            opacity: 0.8;
            cursor: pointer;

            &:active {
              opacity: 0.5;
            }
          }
        }
      }

      .mobile-progress {
        width: 100%;
        margin-bottom: 16px;

        .time-display {
          display: flex;
          justify-content: space-between;
          margin-top: 8px;
          font-size: max(1.2vh, 0.7rem);
          opacity: 0.5;
          color: var(--main-cover-color);
        }
      }

      .mobile-control-buttons {
        display: flex;
        align-items: center;
        justify-content: space-evenly;
        width: 100%;

        .n-icon {
          color: var(--main-cover-color);
          cursor: pointer;
          transition: transform 0.15s ease, opacity 0.15s ease;

          &:active {
            transform: scale(0.85);
          }
        }

        .mode-btn {
          opacity: 0.6;

          &:active {
            transform: scale(0.85);
          }
        }

        .prev,
        .next {
          opacity: 0.9;
        }

        .dislike {
          opacity: 0.9;
        }

        .play-state {
          display: flex;
          align-items: center;
          justify-content: center;

          .n-button {
            --n-width: min(16vw, 64px);
            --n-height: min(16vw, 64px);
            --n-color: transparent;
            --n-color-hover: rgba(255, 255, 255, 0.1);
            --n-color-pressed: rgba(255, 255, 255, 0.15);
            --n-text-color: var(--main-cover-color);
            --n-text-color-hover: var(--main-cover-color);
            --n-text-color-pressed: var(--main-cover-color);
            --n-border: none;
            border: none;
          }

          .n-icon {
            opacity: 1;
          }
        }
      }

      .mobile-volume {
        display: flex;
        align-items: center;
        width: 100%;
        margin-top: 16px;

        :deep(.n-icon) {
          color: var(--main-cover-color);
          opacity: 0.4;
          flex-shrink: 0;
        }
      }
    }

    // ── AMLL .coverFrame ──
    .mobile-cover-frame {
      position: absolute;
      width: 0px;
      height: 0px;
      overflow: hidden;
      cursor: pointer;
      pointer-events: auto;
      z-index: 60;
      box-shadow: 0px 12px 40px rgba(0, 0, 0, 0.35);
      // CSS transition 作为备用（后续替换为 spring 动画）
      transition: width 0.4s ease, height 0.4s ease, left 0.4s ease, top 0.4s ease, border-radius 0.4s ease;

      img {
        position: absolute;
        top: 0;
        left: 0;
        width: 100%;
        height: 100%;
        object-fit: cover;
      }

      &:active {
        opacity: 0.9;
      }
    }

    @keyframes marquee-scroll {
      0% {
        transform: translateX(0);
      }

      100% {
        transform: translateX(-50%);
      }
    }

    // ═══ 状态切换 (AMLL .hideLyric 模式反转) ═══
    // 默认: Layer 1 可见 (= AMLL hideLyric)
    .mobile-small-controls {
      opacity: 0;
      transition: opacity 0.5s;
      pointer-events: none;
    }

    .mobile-cover-layout {
      pointer-events: auto;
    }

    .mobile-lyric,
    .no-lyrics {
      opacity: 0;
      transition: opacity 0.5s;
      pointer-events: none;
    }

    .mobile-big-controls {
      opacity: 1;
    }

    // Layer 2 激活 (= AMLL default)
    &.layer2-active {
      .mobile-small-controls {
        opacity: 1;
        transition: opacity 0.25s 0.25s;
        pointer-events: auto;
      }

      .mobile-cover-layout {
        pointer-events: none;
      }

      .mobile-lyric,
      .no-lyrics {
        opacity: 1;
        transition: opacity 0.5s 0.5s;
        pointer-events: auto;
      }

      .mobile-big-controls {
        opacity: 0;
        pointer-events: none;
      }
    }
  }

  .overlay {
    position: absolute;
    top: 0;
    left: 0;
    width: 100%;
    height: 100%;
    overflow: hidden;
    z-index: -2;
    transition: filter 0.5s ease;
    will-change: filter, opacity;

    &.solid {
      background: var(--cover-bg);
      transition: background 0.8s ease;
    }

    &::after {
      content: "";
      position: absolute;
      top: 0;
      left: 0;
      width: 100%;
      height: 100%;
      background-color: #00000060;
    }

    &.blur {
      display: flex;
      align-items: center;
      justify-content: center;

      .overlay-img {
        width: 150%;
        height: 150%;
        filter: blur(80px) contrast(1.2);
        transition: filter 0.8s ease;
        will-change: filter, transform;
        animation: none !important;
      }

      .blur-webgl {
        position: absolute;
        width: 100%;
        height: 100%;
        top: 0;
        left: 0;
        overflow: hidden;
      }
    }

    &.none {
      &::after {
        display: none;
      }
    }
  }

  .gray {
    position: absolute;
    top: 0;
    left: 0;
    width: 100%;
    height: 100%;
    background-color: #00000030;
    -webkit-backdrop-filter: blur(80px);
    backdrop-filter: blur(80px);
    z-index: -1;
    transition: backdrop-filter 0.5s ease, background-color 0.5s ease;

    &.blur {
      background-color: #00000060;
    }
  }

  &.bplayer-eplor,
  &.bplayer-blur {
    background-color: gray !important;

    .gray {
      backdrop-filter: none !important;
      -webkit-backdrop-filter: none !important;
      transition: none !important;
      background-color: transparent !important;
    }

    .overlay::after {
      background-color: transparent !important;
    }
  }

  .icon-menu {
    padding: 20px;
    width: 100%;
    height: 80px;
    position: absolute;
    top: 0;
    left: 0;
    display: flex;
    mix-blend-mode: plus-lighter;
    align-items: center;
    justify-content: space-between;
    z-index: 5;
    /* 提高层级确保按钮可点击 */
    box-sizing: border-box;

    /* 移动端隐藏，使用专用的 mobile-close-btn */
    @media (max-width: 768px) {
      display: none;
    }

    .menu-left,
    .menu-right {
      display: flex;
      align-items: center;

      .icon {
        width: 40px;
        height: 40px;
        display: flex;
        align-items: center;
        justify-content: center;
        font-size: 40px;
        opacity: 0.3;
        border-radius: 8px;
        transition: all 0.3s cubic-bezier(0.34, 1.56, 0.64, 1);
        cursor: pointer;
        will-change: transform, opacity, background-color;

        &:hover {
          background-color: #ffffff20;
          transform: scale(1.05);
          opacity: 1;
        }

        &:active {
          transform: scale(1);
        }

        .screenfull,
        .setting {
          @media (max-width: 768px) {
            display: none;
          }
        }
      }
    }

    .menu-right {
      .icon {
        margin-left: 12px;
      }
    }
  }

  .all {
    width: 100%;
    height: 100%;
    display: flex;
    flex-direction: row;
    align-items: center;
    position: relative;

    &.noLrc {
      justify-content: center;

      .left {
        padding-right: 0;
        width: auto;
        transform: none;
        align-items: center;
      }
    }

    .tip {
      position: absolute;
      top: 24px;
      left: calc(50% - 150px);
      width: 300px;
      height: 40px;
      border-radius: 25px;
      background-color: #ffffff20;
      -webkit-backdrop-filter: blur(20px);
      backdrop-filter: blur(20px);
      display: flex;
      align-items: center;
      justify-content: center;
      z-index: 4;
      will-change: transform, opacity;

      span {
        color: #ffffffc7;
      }
    }

    .left {
      width: 40%;
      display: flex;
      flex-direction: column;
      align-items: center;
      justify-content: center;
      padding-left: 2rem;
      box-sizing: border-box;
    }

    .right {
      flex: 1;
      height: 100%;
      mix-blend-mode: plus-lighter;
      padding-right: 1rem;

      .lrcShow {
        height: 100%;
        display: flex;
        justify-content: center;
        flex-direction: column;

        .data {
          padding: 0 3vh;
          margin-bottom: 8px;
          text-shadow: 0 0 0.3em color-mix(in srgb, currentColor 15%, transparent);

          .name {
            font-size: 3vh;
            -webkit-line-clamp: 2;
            line-clamp: 2;
            padding-right: 26px;
            will-change: transform, opacity;

            span {
              &:nth-of-type(2) {
                margin-left: 12px;
                font-size: 2.3vh;
                opacity: 0.6;
              }
            }
          }

          .artists {
            margin-top: 4px;
            opacity: 0.6;
            font-size: 1.8vh;
            will-change: transform, opacity;

            .artist {
              span {
                &:nth-of-type(2) {
                  margin: 0 2px;
                }
              }
            }
          }
        }

        .menu {
          opacity: 0;
          padding: 1vh 2vh;
          display: flex !important;
          justify-content: center;
          align-items: center;
          transition: all 0.3s cubic-bezier(0.34, 1.56, 0.64, 1);
          flex-direction: column;
          will-change: opacity;

          .time {
            display: flex;
            flex-direction: row;
            align-items: center;
            width: 100%;
            margin-right: 3em;
            margin-left: 3em;

            span {
              opacity: 0.8;
            }

            .bouncing-slider {
              margin: 0 10px;
              flex: 1;
            }
          }

          .control {
            margin-top: 0.8em;
            display: flex;
            flex-direction: row;
            align-items: center;
            justify-content: center;
            transform: scale(1.4);

            .next,
            .prev,
            .dislike {
              cursor: pointer;
              padding: 4px;
              border-radius: 50%;
              transition: all 0.3s cubic-bezier(0.34, 1.56, 0.64, 1);
              will-change: transform, background-color;

              &:hover {
                background-color: var(--main-color);
                transform: scale(1.1);
              }

              &:active {
                transform: scale(0.9);
              }
            }

            .dislike {
              padding: 9px;
            }

            .play-state {
              --n-width: 42px;
              --n-height: 42px;
              color: var(--main-cover-color);
              margin: 0 12px;
              transition: all 0.3s cubic-bezier(0.34, 1.56, 0.64, 1);
              will-change: transform, background-color;

              .n-icon {
                transition: all 0.2s cubic-bezier(0.34, 1.56, 0.64, 1);
                color: var(--main-cover-color);
                will-change: transform, opacity;
              }

              &:active {
                transform: scale(1);
              }

              &:hover .n-icon {
                transform: scale(1.1);
              }
            }
          }

          &.show {
            opacity: 1;
          }

          .n-icon {
            font-size: 24px;
            cursor: pointer;
            padding: 8px;
            border-radius: 8px;
            opacity: 0.4;
            transition: all 0.3s cubic-bezier(0.34, 1.56, 0.64, 1);
            will-change: transform, opacity, background-color;

            &:hover {
              background-color: #ffffff30;
              transform: scale(1.05);
            }

            &:active {
              transform: scale(0.95);
            }

            &.open {
              opacity: 1;
            }

          }
        }
      }
    }
  }

  .canvas {
    display: flex;
    justify-content: center;
    align-items: flex-end;
    max-width: 1600px;
    z-index: -1;
    position: absolute;
    bottom: 0;
    -webkit-mask: linear-gradient(to right,
        hsla(0deg, 0%, 100%, 0) 0,
        hsla(0deg, 0%, 100%, 0.6) 15%,
        #fff 30%,
        #fff 70%,
        hsla(0deg, 0%, 100%, 0.6) 85%,
        hsla(0deg, 0%, 100%, 0));
    mask: linear-gradient(to right,
        hsla(0deg, 0%, 100%, 0) 0,
        hsla(0deg, 0%, 100%, 0.6) 15%,
        #fff 30%,
        #fff 70%,
        hsla(0deg, 0%, 100%, 0.6) 85%,
        hsla(0deg, 0%, 100%, 0));

    .avBars {
      max-width: 1600px;
      opacity: 0.6;
    }
  }
}

/* 桌面端左侧控制区 plus-lighter — :global 绕过 scoped 组件边界 */
:global(.bplayer .left .controls),
:global(.bplayer .left .controls .bouncing-slider),
:global(.bplayer .left .controls .n-icon) {
  mix-blend-mode: plus-lighter;
}


/* 添加自定义动画 */
@keyframes slowRotate {
  0% {
    transform: rotate(0deg);
  }

  100% {
    transform: rotate(360deg);
  }
}

/* 更新CSS过渡效果 */
.fade-enter-active,
.fade-leave-active {
  transition: opacity 0.3s cubic-bezier(0.34, 1.56, 0.64, 1);
}

.fade-enter-from,
.fade-leave-to {
  opacity: 0;
}

.control {

  .prev,
  .next {
    width: 30px;
    height: 30px;
  }

  .control-icon {
    width: 42px;
    height: 42px;
  }
}
</style>
