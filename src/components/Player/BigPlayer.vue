<template>
  <div
    ref="bigPlayerRef"
    v-show="music.showBigPlayer"
    :class="[
      'bplayer',
      `bplayer-${setting.backgroundImageShow}`,
      isMobile ? 'mobile-player' : 'desktop-player',
      setting.appleStyle && !isMobile ? 'apple-style' : ''
    ]"
    :style="[
      `--cover-bg: ${songPicGradient}`,
      `--main-cover-color: rgb(${setting.immersivePlayer ? songPicColor : '255,255,255'})`,
    ]"
  >
    <!-- 共用部分: 背景和顶部菜单 -->
    <Transition name="fade" mode="out-in">
      <div :key="`bg--${songPicGradient}`" :class="['overlay', setting.backgroundImageShow]">
        <template v-if="setting.backgroundImageShow === 'blur'">
          <BlurBackgroundRender
            v-if="music.getPlaySongData"
            :fps="music.getPlayState ? setting.fps || 30 : 0"
            :playing="actualPlayingProp"
            :album="music.getPlaySongData.album.picUrl.replace(/^http:/, 'https:')"
            :blurLevel="setting.blurAmount || 30"
            :saturation="setting.contrastAmount || 1.2"
            :renderScale="setting.renderScale || 0.5"
            class="blur-webgl"
          />
        </template>
      </div>
    </Transition>

    <template v-if="setting.backgroundImageShow == 'eplor'">
      <BackgroundRender 
        :fps="music.getPlayState ? setting.fps : 0"
        :playing="actualPlayingProp"
        :flowSpeed="music.getPlayState ? setting.flowSpeed : 0"
        :album="setting.albumImageUrl === 'none' ? music.getPlaySongData.album.picUrl.replace(/^http:/, 'https:') : setting.albumImageUrl"
        :renderScale="setting.renderScale" 
        :lowFreqVolume="computedLowFreqVolume"
        :staticMode="!music.showBigPlayer"
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

    <!-- 移动端布局 - 重新设计的三层结构 -->
    <template v-if="isMobile">
      <!-- 抽屉把手：顶部居中横条 -->
      <div class="mobile-drawer-handle" @click="closeBigPlayer">
        <div class="handle-bar"></div>
      </div>

      <!-- 顶部 Header 区域：仅封面 -->
      <div
        ref="mobileHeaderRef"
        class="mobile-header"
        :class="{ 'is-compact': mobileLayer === 2 }"
      >
        <!-- 封面 -->
        <div class="mobile-cover" @click="switchMobileLayer(mobileLayer === 1 ? 2 : 1)">
          <img
            :src="music.getPlaySongData ? music.getPlaySongData.album.picUrl.replace(/^http:/, 'https:') + '?param=500y500' : '/images/pic/default.png'"
            alt="cover"
          />
        </div>
      </div>

      <!-- 歌曲信息行：绝对定位，统一用 top 实现平滑过渡 -->
      <div class="mobile-song-info-row" :class="{ 'is-compact': mobileLayer === 2 }">
        <div class="mobile-song-info">
          <div class="name-wrapper" ref="nameWrapperRef">
            <div class="name" ref="nameTextRef" :class="{ 'is-scrolling': isNameOverflow }">
              <span class="name-inner">{{ music.getPlaySongData ? music.getPlaySongData.name : $t("other.noSong") }}</span>
              <span class="name-inner" v-if="isNameOverflow">{{ music.getPlaySongData ? music.getPlaySongData.name : $t("other.noSong") }}</span>
            </div>
          </div>
          <div class="artists text-hidden" v-if="music.getPlaySongData && music.getPlaySongData.artist">
            <span v-for="(item, index) in music.getPlaySongData.artist" :key="item">
              {{ item.name }}<span v-if="index != music.getPlaySongData.artist.length - 1"> / </span>
            </span>
          </div>
        </div>
        <!-- 操作按钮 -->
        <div class="mobile-header-actions">
          <n-icon size="24" :component="StarBorderRound" @click.stop="
            music.getPlaySongData && (
              music.getSongIsLike(music.getPlaySongData.id)
                ? music.changeLikeList(music.getPlaySongData.id, false)
                : music.changeLikeList(music.getPlaySongData.id, true)
            )
          " />
          <n-icon size="24" :component="MoreVertRound" @click.stop="" />
        </div>
      </div>

      <!-- 歌词区域 - 仅在 Layer 2 显示 -->
      <div
        ref="mobileLyricsRef"
        class="mobile-lyrics-area"
        :class="{ 'is-visible': mobileLayer === 2, 'is-expanded': lyricsExpanded }"
        @click.capture="handleLyricsAreaClick"
      >
        <div
          class="mobile-lyrics-container"
          v-if="music.getPlaySongLyric && music.getPlaySongLyric.lrc &&
            music.getPlaySongLyric.lrc[0] &&
            music.getPlaySongLyric.lrc.length > 4"
          @scroll="handleLyricsScroll"
          @touchmove="handleLyricsScroll"
        >
          <RollingLyrics
            @mouseenter="lrcMouseStatus = setting.lrcMousePause ? true : false"
            @mouseleave="lrcAllLeave"
            @lrcTextClick="lrcTextClick"
            class="mobile-lyrics"
          ></RollingLyrics>
        </div>
        <div v-else class="no-lyrics">
          <span>{{ $t("other.noLyrics") }}</span>
        </div>
      </div>

      <!-- 底部 Controls 区域 -->
      <div
        ref="mobileControlsRef"
        class="mobile-controls"
        :class="{ 'is-hidden': !mobileControlsVisible }"
      >
        <!-- 进度条 -->
        <div class="mobile-progress">
          <vue-slider v-model="music.getPlaySongTime.barMoveDistance" @drag-start="music.setPlayState(false)"
            @drag-end="sliderDragEnd" @click.stop="songTimeSliderUpdate(music.getPlaySongTime.barMoveDistance)"
            :tooltip="'none'" />
          <div class="time-display">
            <span>{{ music.getPlaySongTime.songTimePlayed }}</span>
            <span>-{{ remainingTime }}</span>
          </div>
        </div>

        <!-- 控制按钮 -->
        <div class="mobile-control-buttons">
          <n-icon class="mode-btn" size="24" :component="persistData.playSongMode === 'random' ? ShuffleOne : persistData.playSongMode === 'single' ? PlayOnce : PlayCycle"
            @click.stop="music.setPlaySongMode()" />
          <n-icon v-if="!music.getPersonalFmMode" class="prev" size="36" :component="IconRewind"
            @click.stop="music.setPlaySongIndex('prev')" />
          <n-icon v-else class="dislike" size="28" :component="ThumbDownRound"
            @click="music.setFmDislike(music.getPersonalFmData.id)" />
          <div class="play-state">
            <n-icon size="64" :component="music.getPlayState ? IconPause : IconPlay"
              @click.stop="music.setPlayState(!music.getPlayState)" />
          </div>
          <n-icon class="next" size="36" :component="IconForward"
            @click.stop="music.setPlaySongIndex('next')" />
          <n-icon class="mode-btn" size="24" :component="PlayCycle"
            @click.stop="music.setPlaySongMode()" />
        </div>

        <!-- 音量控制 -->
        <div class="mobile-volume">
          <n-icon size="20" :component="VolumeOffRound" />
          <vue-slider v-model="persistData.playVolume" :min="0" :max="1" :interval="0.01" :tooltip="'none'" />
          <n-icon size="20" :component="VolumeUpRound" />
        </div>
      </div>
    </template>

    <!-- 桌面端布局 -->
    <template v-else>
      <div :class="[
        music.getPlaySongLyric && music.getPlaySongLyric.lrc && music.getPlaySongLyric.lrc[0] && music.getPlaySongLyric.lrc.length > 4 && !music.getLoadingState
          ? 'all'
          : 'all noLrc',
        setting.appleStyle ? 'apple-layout' : ''
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
            <div class="data" v-show="setting.playerStyle === 'record' || setting.appleStyle">
              <div class="name text-hidden">
                <span>{{
                  music.getPlaySongData
                    ? music.getPlaySongData.name
                    : $t("other.noSong")
                }}</span>
                <span v-if="music.getPlaySongData && music.getPlaySongData.alia">{{ music.getPlaySongData.alia[0]
                  }}</span>
              </div>
              <div class="artists text-hidden" v-if="music.getPlaySongData && music.getPlaySongData.artist">
                <span class="artist" v-for="(item, index) in music.getPlaySongData.artist" :key="item">
                  <span>{{ item.name }}</span>
                  <span v-if="index != music.getPlaySongData.artist.length - 1">/</span>
                </span>
              </div>
            </div>
            
            <RollingLyrics 
              @mouseenter="lrcMouseStatus = setting.lrcMousePause ? true : false" 
              @mouseleave="lrcAllLeave" 
              @lrcTextClick="lrcTextClick"
              :class="setting.appleStyle ? 'apple-lyrics' : ''"
            ></RollingLyrics>
            
            <div :class="[(menuShow || setting.appleStyle) ? 'menu show' : 'menu', setting.appleStyle ? 'apple-controls' : '']" 
              v-show="setting.playerStyle === 'record' || setting.appleStyle">
              <div class="time">
                <span>{{ music.getPlaySongTime.songTimePlayed }}</span>
                <vue-slider v-model="music.getPlaySongTime.barMoveDistance" @drag-start="music.setPlayState(false)"
                  @drag-end="sliderDragEnd" @click.stop="
                    songTimeSliderUpdate(music.getPlaySongTime.barMoveDistance)
                    " :tooltip="'none'" />
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
</template>

<script setup>
import {
  KeyboardArrowDownFilled,
  FullscreenRound,
  FullscreenExitRound,
  SettingsRound,
  ThumbDownRound,
  StarBorderRound,
  MoreVertRound,
  VolumeUpRound,
  VolumeOffRound,
  VolumeMuteRound,
} from "@vicons/material";
import { ShuffleOne, PlayOnce, PlayCycle } from "@icon-park/vue-next";
import { musicStore, settingStore, siteStore } from "@/store";
import { useRouter } from "vue-router";
import { setSeek } from "@/utils/Player";
import PlayerRecord from "./PlayerRecord.vue";
import PlayerCover from "./PlayerCover.vue";
import RollingLyrics from "./RollingLyrics.vue";
import Spectrum from "./Spectrum.vue";
import LyricSetting from "@/components/DataModal/LyricSetting.vue";
import screenfull from "screenfull";
import VueSlider from "vue-slider-component";
import "vue-slider-component/theme/default.css";
import BackgroundRender from "@/libs/apple-music-like/BackgroundRender.vue";
import { throttle } from "throttle-debounce";
import { LowFreqVolumeAnalyzer } from "../../utils/lowFreqVolumeAnalyzer";
import { storeToRefs } from "pinia";
import gsap from "gsap";
import {
  onMounted,
  nextTick,
  watch,
  ref,
  shallowRef,
  computed,
  onBeforeUnmount,
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

// 为设置添加Apple Music样式选项
if (typeof setting.appleStyle === 'undefined') {
  setting.$patch({
    appleStyle: true
  });
}

const { songPicGradient, songPicColor } = storeToRefs(site)
const { persistData } = storeToRefs(music)

// 创建需要的refs用于GSAP动画
const bigPlayerRef = ref(null);
const tipRef = ref(null);
const leftContentRef = ref(null);
const rightContentRef = ref(null);
const lowFreqVolume = shallowRef(1.0);

// Create low-frequency volume analyzer instance
const lowFreqAnalyzer = new LowFreqVolumeAnalyzer();

// Watch for spectrum data to calculate low-frequency volume for background rendering
// This replaces the previous dynamicFlowSpeed approach with direct lowFreqVolume control
watch(() => music.getSpectrumsData, throttle(100, (val) => {
  if (!music.getPlayState || !val) {
    return;
  }

  // Use the new analyzer to calculate smoothed low-frequency volume
  lowFreqVolume.value = lowFreqAnalyzer.analyze(val);
}));

// 检测是否为移动设备
const isMobile = ref(false);

// 移动端当前显示层级 (1=控制层, 2=歌词层)
const mobileLayer = ref(1);

// 移动端元素引用
const mobileControlsRef = ref(null);
const nameWrapperRef = ref(null);
const nameTextRef = ref(null);

// 移动端 controls 可见状态
const mobileControlsVisible = ref(true);

// 歌曲名称是否溢出（需要滚动）
const isNameOverflow = ref(false);

// 歌词是否已展开（用户滑动后）
const lyricsExpanded = ref(false);

// 计算剩余时间 (负数 ETA)
const remainingTime = computed(() => {
  const playTime = music.getPlaySongTime;
  if (!playTime || !playTime.duration) return '0:00';

  const remaining = Math.max(0, playTime.duration - (playTime.currentTime || 0));
  const minutes = Math.floor(remaining / 60);
  const seconds = Math.floor(remaining % 60);
  return `${minutes}:${seconds.toString().padStart(2, '0')}`;
});

// 计算 lowFreqVolume 的最终值，优化性能
const computedLowFreqVolume = computed(() => {
  return setting.dynamicFlowSpeed ? Number((Math.round(lowFreqVolume.value * 100) / 100).toFixed(2)) : 1.0;
});

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

  // 重置状态
  mobileLayer.value = 1;
  lyricsExpanded.value = false;
  mobileControlsVisible.value = true;

  nextTick(() => {
    const controls = mobileControlsRef.value;
    // 初始化 controls 状态
    if (controls) {
      gsap.set(controls, { opacity: 1, y: 0 });
    }
    // 检测名称是否溢出
    checkNameOverflow();
  });
};

// 切换移动端层级 - 使用 CSS transition 实现动画
const switchMobileLayer = (targetLayer) => {
  if (targetLayer === mobileLayer.value) return;

  const controls = mobileControlsRef.value;

  // 切回 layer 1 时，重置状态
  if (targetLayer === 1) {
    lyricsExpanded.value = false;
    mobileControlsVisible.value = true;
    if (controls) {
      gsap.fromTo(controls,
        { opacity: 0, y: 30 },
        { opacity: 1, y: 0, duration: 0.4, ease: 'power2.out' }
      );
    }
  }

  // 切换层
  mobileLayer.value = targetLayer;
};

// 歌词区域滑动开始，展开歌词并隐藏 controls
const handleLyricsScroll = () => {
  if (!isMobile.value || mobileLayer.value !== 2 || lyricsExpanded.value) return;

  lyricsExpanded.value = true;
  const controls = mobileControlsRef.value;
  if (controls && mobileControlsVisible.value) {
    mobileControlsVisible.value = false;
    gsap.to(controls, {
      opacity: 0,
      y: 30,
      duration: 0.3,
      ease: 'power2.in'
    });
  }
};

// 点击歌词区域，切换 controls 显示状态
const handleLyricsAreaClick = () => {
  if (!isMobile.value || mobileLayer.value !== 2) return;

  const controls = mobileControlsRef.value;
  if (!controls) return;

  // 如果 controls 隐藏，则显示；如果已展开歌词则显示 controls
  if (!mobileControlsVisible.value) {
    mobileControlsVisible.value = true;
    lyricsExpanded.value = false;
    gsap.fromTo(controls,
      { opacity: 0, y: 30 },
      { opacity: 1, y: 0, duration: 0.3, ease: 'power2.out' }
    );
  }
};

// 检测是否页面上已有标题组件
const hasHeaderComponent = ref(false);

// 检测视窗尺寸变化，更新移动设备状态
const updateDeviceStatus = () => {
  isMobile.value = window.innerWidth <= 768;
  console.log("isMobile", isMobile.value);
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

// 关闭大播放器
const closeBigPlayer = () => {
  if (setting.appleStyle && !isMobile.value) {
    // Apple Music风格的退出动画
    gsap.to(bigPlayerRef.value, {
      opacity: 0,
      scale: 1.05,
      duration: 0.5,
      ease: "sine.in",
      onComplete: () => {
        music.setBigPlayerState(false);
      }
    });
  } else {
    // 原来的动画
    gsap.to(bigPlayerRef.value, {
      y: window.innerHeight, 
      opacity: 0,
      duration: 0.5,
      ease: "power2.inOut",
      onComplete: () => {
        music.setBigPlayerState(false);
      }
    });
  }
};

// 歌词文本点击事件
const lrcTextClick = (time) => {
  if (typeof $player !== "undefined") {
    // 防止soundStop被调用
    music.persistData.playSongTime.currentTime = time;
    $player.seek(time);
    music.setPlayState(true);
  }
  lrcMouseStatus.value = false;
};

// 歌曲进度条更新
const sliderDragEnd = () => {
  songTimeSliderUpdate(music.getPlaySongTime.barMoveDistance);
  music.setPlayState(true);
  
  // 添加进度条拖动结束后的动画效果
  const sliderEl = document.querySelector('.vue-slider-dot');
  if (sliderEl) {
    gsap.fromTo(sliderEl, 
      { scale: 1.3 },
      { scale: 1, duration: 0.3, ease: "elastic.out(1, 0.3)" }
    );
  }
};
const songTimeSliderUpdate = (val) => {
  if (typeof $player !== "undefined" && music.getPlaySongTime?.duration) {
    const currentTime = (music.getPlaySongTime.duration / 100) * val;
    setSeek($player, currentTime);
  }
};

// 鼠标移出歌词区域
const lrcMouseStatus = ref(false);
const lrcAllLeave = () => {
  lrcMouseStatus.value = false;
  lyricsScroll(music.getPlaySongLyricIndex);
};

// 使用GSAP动画显示提示文本
const animateTip = (isVisible) => {
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
const lyricsScroll = (index) => {
  const type = setting.lyricsBlock;
  const lrcType =
    !music.getPlaySongLyric.hasYrc || !setting.showYrc ? "lrc" : "yrc";
  const el = document.getElementById(lrcType + index);
  
  if (!el || lrcMouseStatus.value) return;
  
  // 获取歌词容器元素
  const container = el.parentElement;
  if (!container) return;
  
  const containerHeight = container.clientHeight;
  
  // 为移动端和桌面端使用不同的滚动计算方式
  let scrollDistance;
  
  if (isMobile.value) {
    // 移动端滚动位置偏上，使活跃歌词在屏幕中上部显示
    scrollDistance = el.offsetTop - container.offsetTop - containerHeight * 0.35;
  } else if (setting.appleStyle) {
    // Apple Music风格的歌词居中显示
    scrollDistance = el.offsetTop - container.offsetTop - containerHeight / 2 + el.offsetHeight / 2;
  } else {
    // 统一桌面端与移动端的滚动逻辑，使其滚动到视口约 35% 的位置
    scrollDistance = el.offsetTop - container.offsetTop - containerHeight * 0.35;
  }
  
  // 使用GSAP动画滚动
  gsap.to(container, {
    scrollTop: scrollDistance,
    duration: setting.appleStyle ? 0.7 : 0.5,
    ease: setting.appleStyle ? "circ.out" : "cubic-bezier(0.34, 1.56, 0.64, 1)"
  });
  
  // 添加当前歌词的强调动画
  if (setting.appleStyle) {
    // 重置所有歌词项
    const allLyrics = container.querySelectorAll('.lrc-item');
    allLyrics.forEach(item => {
      if (item !== el) {
        gsap.to(item, {
          scale: 1,
          opacity: 0.7,
          fontWeight: 400,
          duration: 0.3,
          ease: "sine.out"
        });
      }
    });
    
    // 激活当前歌词
    gsap.fromTo(el, 
      { scale: 0.95, opacity: 0.7 },
      { 
        scale: 1.02, 
        opacity: 1, 
        fontWeight: 600,
        duration: 0.5, 
        ease: "sine.out",
        onComplete: () => {
          // 添加脉动效果
          gsap.to(el, {
            scale: 1,
            duration: 1.2,
            repeat: 1,
            yoyo: true,
            ease: "sine.inOut"
          });
        }
      }
    );
  } else {
    // 原来的动画
    gsap.fromTo(el, 
      { scale: 0.95, opacity: 0.7 },
      { scale: 1, opacity: 1, duration: 0.3, ease: "back.out(1.7)" }
    );
  }
};

// 改变 PWA 应用标题栏颜色
const changePwaColor = () => {
  const themeColorMeta = document.querySelector('meta[name="theme-color"]');
  if (music.showBigPlayer) {
    themeColorMeta.setAttribute("content", songPicColor);
  } else {
    if (setting.getSiteTheme === "light") {
      themeColorMeta.setAttribute("content", "#ffffff");
    } else if (setting.getSiteTheme === "dark") {
      themeColorMeta.setAttribute("content", "#18181c");
    }
  }
};

// 使用GSAP动画显示播放器，为Apple风格添加特殊处理
const animatePlayerIn = () => {
  if (!bigPlayerRef.value) return;
  
  if (setting.appleStyle && !isMobile.value) {
    // Apple Music风格的入场动画
    
    // 主容器动画
    gsap.fromTo(bigPlayerRef.value, 
      { opacity: 0, scale: 1.05 },
      { 
        opacity: 1, 
        scale: 1,
        duration: 0.8, 
        ease: "sine.out"
      }
    );
    
    // 左侧专辑封面动画
    if (leftContentRef.value) {
      gsap.fromTo(leftContentRef.value,
        { opacity: 0, x: -60, rotateY: "-40deg" },
        { 
          opacity: 1, 
          x: 0,
          rotateY: "0deg",
          duration: 1.2, 
          delay: 0.1,
          ease: "elastic.out(1, 0.8)" 
        }
      );
    }
    
    // 右侧内容动画 - 歌曲信息
    const songInfo = document.querySelector('.apple-layout .data');
    if (songInfo) {
      gsap.fromTo(songInfo,
        { opacity: 0, y: -30 },
        { 
          opacity: 1, 
          y: 0,
          duration: 0.7, 
          delay: 0.2,
          ease: "sine.out" 
        }
      );
    }
    
    // 右侧内容动画 - 歌词
    const lyrics = document.querySelector('.apple-lyrics');
    if (lyrics) {
      gsap.fromTo(lyrics,
        { opacity: 0 },
        { 
          opacity: 1,
          duration: 0.7, 
          delay: 0.3,
          ease: "sine.out" 
        }
      );
    }
    
    // 右侧内容动画 - 控制条
    const controls = document.querySelector('.apple-controls');
    if (controls) {
      gsap.fromTo(controls,
        { opacity: 0, y: 30 },
        { 
          opacity: 1, 
          y: 0,
          duration: 0.7, 
          delay: 0.4,
          ease: "sine.out" 
        }
      );
    }
  } else {
    // 原来的动画
    // 主容器动画
    gsap.fromTo(bigPlayerRef.value, 
      { opacity: 0, y: window.innerHeight },
      { 
        opacity: 1, 
        y: 0, 
        duration: 0.5, 
        ease: "cubic-bezier(0.34, 1.56, 0.64, 1)" // 使用贝塞尔曲线
      }
    );
    
    if (isMobile.value) {
      // 移动端动画
      if (rightContentRef.value) {
        gsap.fromTo(rightContentRef.value,
          { opacity: 0, y: 50 },
          { 
            opacity: 1, 
            y: 0,
            duration: 0.6, 
            delay: 0.2,
            ease: "power2.out" 
          }
        );
      }
    } else {
      // 桌面端动画
      // 左侧内容动画
      if (leftContentRef.value) {
        gsap.fromTo(leftContentRef.value,
          { opacity: 0, x: -50 },
          { 
            opacity: 1, 
            x: 0, 
            duration: 0.6, 
            delay: 0.2,
            ease: "power2.out" 
          }
        );
      }
      
      // 右侧内容动画
      if (rightContentRef.value) {
        gsap.fromTo(rightContentRef.value,
          { opacity: 0, x: 50 },
          { 
            opacity: 1, 
            x: 0,
            duration: 0.6, 
            delay: 0.3,
            ease: "power2.out" 
          }
        );
      }
    }
  }
};

onMounted(() => {
  console.log("BigPlayer onMounted - forcePlaying initially:", forcePlaying.value);

  // 初始化设备检测
  updateDeviceStatus();
  window.addEventListener('resize', updateDeviceStatus);

  // 检测页面上是否已有标题组件
  checkHeaderComponent();

  // 初始化GSAP
  gsap.config({
    force3D: true,
    nullTargetWarn: false
  });

  // 初始化移动端共享元素位置
  initMobileElements();
  
  // 使用GSAP创建播放按钮动画效果
  const setupButtonAnimations = () => {
    const buttons = document.querySelectorAll('.n-icon.prev, .n-icon.next, .play-state');
    buttons.forEach(button => {
      button.addEventListener('mouseenter', () => {
        gsap.to(button, { scale: 1.1, duration: 0.2, ease: "back.out(1.7)" });
      });
      button.addEventListener('mouseleave', () => {
        gsap.to(button, { scale: 1, duration: 0.2, ease: "power1.out" });
      });
    });
    
    // Apple风格特定动画
    if (setting.appleStyle && !isMobile.value) {
      // 为歌曲信息添加呼吸效果
      const songInfo = document.querySelector('.apple-layout .data');
      if (songInfo) {
        gsap.to(songInfo, {
          opacity: 0.85,
          duration: 3,
          repeat: -1,
          yoyo: true,
          ease: "sine.inOut"
        });
      }
      
      // 为活跃歌词添加脉动效果
      const setLyricPulse = () => {
        const activeLyric = document.querySelector('.apple-lyrics .lrc-item.active');
        if (activeLyric) {
          gsap.to(activeLyric, {
            scale: 1.02,
            opacity: 1,
            duration: 1.2,
            repeat: -1,
            yoyo: true,
            ease: "sine.inOut"
          });
        }
      };
      
      // 监听歌词变化以应用动画
      const observer = new MutationObserver(() => {
        setLyricPulse();
      });
      
      const lyricContainer = document.querySelector('.apple-lyrics');
      if (lyricContainer) {
        observer.observe(lyricContainer, { 
          childList: true,
          subtree: true,
          attributes: true,
          attributeFilter: ['class']
        });
        
        // 初始设置
        setLyricPulse();
      }
    }
  };
  
  nextTick(() => {
    console.log(
      "BigPlayer nextTick starts - forcePlaying BEFORE change:",
      forcePlaying.value
    );
    // After the first tick, disable the forcing flag
    forcePlaying.value = false;
    console.log(
      "BigPlayer nextTick ends - forcePlaying AFTER change:",
      forcePlaying.value
    );

    // Existing logic from nextTick
    if (setting.backgroundImageShow === "eplor") {
      console.log("Eplor mode active on mount.");
    }
    lyricsScroll(music.getPlaySongLyricIndex);
    
    // 添加按钮动画初始化
    setupButtonAnimations();
  });
});

onBeforeUnmount(() => {
  clearTimeout(timeOut.value);
  window.removeEventListener('resize', updateDeviceStatus);
  // Reset the low-frequency volume analyzer
  lowFreqAnalyzer.reset();
});

// 监听页面是否打开
watch(
  () => music.showBigPlayer,
  (val) => {
    changePwaColor();
    if (val) {
      console.log("开启播放器", music.getPlaySongLyricIndex);
      // 重新检测页面上是否已有标题组件
      checkHeaderComponent();
      // 初始化移动端共享元素位置
      initMobileElements();
      nextTick().then(() => {
        music.showPlayList = false;
        lyricsScroll(music.getPlaySongLyricIndex);
        animatePlayerIn(); // 添加GSAP入场动画
      });
    }
  }
);

// 监听移动设备状态变化
watch(
  () => isMobile.value,
  () => {
    // 设备状态变化时，重新计算歌词滚动位置
    nextTick(() => {
      lyricsScroll(music.getPlaySongLyricIndex);
    });
  }
);

// 监听歌词提示状态
watch(
  () => lrcMouseStatus.value,
  (val) => {
    animateTip(val);
  }
);

// 监听歌词滚动
watch(
  () => music.getPlaySongLyricIndex,
  (val) => lyricsScroll(val)
);

// 监听主题色改变
watch(
  () => site.songPicColor,
  () => changePwaColor()
);

// 监听歌曲变化，检测名称是否溢出
watch(
  () => music.getPlaySongData,
  () => {
    checkNameOverflow();
  },
  { immediate: true }
);
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
  transition: background 0.5s ease;
  will-change: transform, opacity, background;

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
        justify-content: flex-end;
        align-items: center;
        
        // 增强封面效果
        :deep(.cover-container), :deep(.record-container) {
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
            
            :deep(.lrc-content) {
              padding: 15vh 0;
              text-align: left;
              width: 100%;
              box-sizing: border-box;
              margin: 0;
            }
            
            :deep(.lrc-item) {
              font-size: 1.2rem;
              line-height: 1.8;
              margin: 12px 0;
              padding: 0;
              letter-spacing: -0.01em;
              font-weight: 400;
              opacity: 0.7;
              transition: all 0.3s cubic-bezier(0.34, 1.56, 0.64, 1);
              
              &.active {
                font-size: 1.5rem;
                font-weight: 600;
                opacity: 1;
                color: white;
                text-shadow: 0 0 10px rgba(255, 255, 255, 0.2);
              }
            }
          }
          
          .apple-controls {
            position: absolute;
            bottom: 6vh;
            left: 17.5%;
            transform: translateX(-50%);
            max-width: 340px;
            width: 90%;
            margin: 0;
            padding: 0;
            background: transparent;
            box-shadow: none;
          }

          .apple-controls .time {
            width: 100%;
            margin: 0 0 12px 0;
          }
        }
      }
    }
  }

  /* 移动端样式 - 三层结构 */
  &.mobile-player {
    display: flex;
    flex-direction: column;

    /* 抽屉把手 */
    .mobile-drawer-handle {
      position: absolute;
      top: calc(env(safe-area-inset-top) + 8px);
      left: 50%;
      transform: translateX(-50%);
      width: 60px;
      height: 20px;
      display: flex;
      align-items: center;
      justify-content: center;
      z-index: 70;
      cursor: pointer;

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

    /* 顶部 Header 区域：仅包含封面 */
    .mobile-header {
      position: absolute;
      top: 0;
      left: 0;
      right: 0;
      z-index: 60;
      pointer-events: none;

      /* 封面：绝对定位，直接用 left 定位实现平滑缩放 */
      .mobile-cover {
        --cover-size: min(70vw, 280px);
        position: absolute;
        width: var(--cover-size);
        height: var(--cover-size);
        left: calc(50% - var(--cover-size) / 2);
        top: calc(env(safe-area-inset-top) + 80px);
        border-radius: 12px;
        overflow: hidden;
        box-shadow: 0 20px 50px rgba(0, 0, 0, 0.4);
        cursor: pointer;
        pointer-events: auto;
        transition: width 0.5s cubic-bezier(0.4, 0, 0.2, 1),
                    height 0.5s cubic-bezier(0.4, 0, 0.2, 1),
                    left 0.5s cubic-bezier(0.4, 0, 0.2, 1),
                    top 0.5s cubic-bezier(0.4, 0, 0.2, 1),
                    border-radius 0.5s cubic-bezier(0.4, 0, 0.2, 1),
                    box-shadow 0.5s cubic-bezier(0.4, 0, 0.2, 1);

        img {
          width: 100%;
          height: 100%;
          object-fit: cover;
        }

        &:active {
          opacity: 0.9;
        }
      }

      /* Layer 2: 紧凑模式 */
      &.is-compact {
        .mobile-cover {
          width: 56px;
          height: 56px;
          left: 16px;
          top: calc(env(safe-area-inset-top) + 36px);
          border-radius: 8px;
          box-shadow: 0 4px 12px rgba(0, 0, 0, 0.3);
        }
      }
    }

    /* 歌曲信息行：绝对定位，统一用 top 实现平滑过渡 */
    .mobile-song-info-row {
      position: absolute;
      left: 24px;
      right: 24px;
      top: calc(100% - 260px);
      z-index: 55;
      display: flex;
      align-items: center;
      justify-content: space-between;
      transition: left 0.5s cubic-bezier(0.4, 0, 0.2, 1),
                  right 0.5s cubic-bezier(0.4, 0, 0.2, 1),
                  top 0.5s cubic-bezier(0.4, 0, 0.2, 1);

      .mobile-song-info {
        flex: 1;
        min-width: 0;
        overflow: hidden;

        .name-wrapper {
          overflow: hidden;
          width: 100%;

          .name {
            display: flex;
            font-size: 1.2rem;
            font-weight: 600;
            color: var(--main-cover-color);
            margin-bottom: 4px;
            white-space: nowrap;
            transition: font-size 0.5s cubic-bezier(0.4, 0, 0.2, 1);

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
          transition: font-size 0.5s cubic-bezier(0.4, 0, 0.2, 1);
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

      /* Layer 2: 紧凑模式 - 移动到封面右侧 */
      &.is-compact {
        left: calc(16px + 56px + 12px);
        right: 16px;
        top: calc(env(safe-area-inset-top) + 36px);

        .mobile-song-info {
          .name-wrapper .name {
            font-size: 0.95rem;
          }

          .artists {
            font-size: 0.75rem;
          }
        }

        .mobile-header-actions {
          gap: 12px;
        }
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

    /* 歌词区域 - 仅在 Layer 2 显示 */
    .mobile-lyrics-area {
      position: absolute;
      top: calc(env(safe-area-inset-top) + 12px + 56px + 12px);
      left: 0;
      right: 0;
      bottom: 220px;
      z-index: 30;
      opacity: 0;
      visibility: hidden;
      transform: translateY(20px);
      pointer-events: none;
      overflow: visible;
      -ms-overflow-style: none;
      scrollbar-width: none;
      ::-webkit-scrollbar {
        display: none;
      }
      transition: opacity 0.4s ease,
                  transform 0.4s ease,
                  bottom 0.4s ease,
                  visibility 0s 0.4s;

      &.is-visible {
        opacity: 1;
        visibility: visible;
        transform: translateY(0);
        pointer-events: auto;
        transition: opacity 0.4s ease,
                    transform 0.4s ease,
                    bottom 0.4s ease,
                    visibility 0s 0s;
      }

      &.is-expanded {
        bottom: 0;
      }

      .mobile-lyrics-container {
        height: 100%;
        overflow-y: auto;

        .mobile-lyrics {
          height: 100%;
          overflow-y: auto;
          padding: 0;

          :deep(.lrc-content) {
            padding: 5vh 0 50vh 0;
          }

          :deep(.lrc-item) {
            font-size: 1.1rem;
            line-height: 1.7;
            margin: 14px 0;
            padding: 4px 0;
            color: var(--main-cover-color);
            opacity: 0.5;
            transition: all 0.3s cubic-bezier(0.4, 0, 0.2, 1);

            &.active {
              font-size: 1.25rem;
              font-weight: 600;
              opacity: 1;
              transform: scale(1.02);
            }
          }
        }
      }

      .no-lyrics {
        display: flex;
        align-items: center;
        justify-content: center;
        height: 100%;

        span {
          font-size: 1rem;
          color: var(--main-cover-color);
          opacity: 0.5;
        }
      }
    }

    /* 底部 Controls 区域 - 透明简洁风格 */
    .mobile-controls {
      position: absolute;
      left: 0;
      right: 0;
      bottom: 0;
      z-index: 50;
      padding: 16px 24px;
      padding-bottom: calc(env(safe-area-inset-bottom) + 24px);
      transition: opacity 0.3s ease, transform 0.3s ease;

      &.is-hidden {
        opacity: 0;
        transform: translateY(30px);
        pointer-events: none;
      }

      .mobile-progress {
        width: 100%;
        margin-bottom: 24px;

        .vue-slider {
          width: 100% !important;
          height: 3px !important;

          :deep(.vue-slider-rail) {
            background-color: rgba(255, 255, 255, 0.2);
            border-radius: 2px;

            .vue-slider-process {
              background-color: var(--main-cover-color) !important;
              border-radius: 2px;
            }
          }
        }

        .time-display {
          display: flex;
          justify-content: space-between;
          margin-top: 8px;
          font-size: 0.7rem;
          opacity: 0.5;
          color: var(--main-cover-color);
        }
      }

      .mobile-control-buttons {
        display: flex;
        align-items: center;
        justify-content: center;
        width: 100%;
        gap: 40px;

        .n-icon {
          color: var(--main-cover-color);
          cursor: pointer;
          transition: transform 0.15s ease, opacity 0.15s ease;

          &:active {
            transform: scale(0.85);
          }
        }

        .mode-btn {
          display: none;
        }

        .prev, .next {
          opacity: 0.9;
        }

        .dislike {
          opacity: 0.9;
        }

        .play-state {
          display: flex;
          align-items: center;
          justify-content: center;

          .n-icon {
            opacity: 1;
          }
        }
      }

      .mobile-volume {
        display: flex;
        align-items: center;
        width: 100%;
        gap: 12px;
        margin-top: 24px;

        .n-icon {
          color: var(--main-cover-color);
          opacity: 0.4;
          flex-shrink: 0;
        }

        .vue-slider {
          flex: 1;
          height: 3px !important;

          :deep(.vue-slider-rail) {
            background-color: rgba(255, 255, 255, 0.2);
            border-radius: 2px;

            .vue-slider-process {
              background-color: var(--main-cover-color) !important;
              border-radius: 2px;
            }

            .vue-slider-dot {
              width: 14px !important;
              height: 14px !important;

              .vue-slider-dot-handle {
                background-color: var(--main-cover-color) !important;
                box-shadow: none;
              }
            }
          }
        }
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
    align-items: center;
    justify-content: space-between;
    z-index: 5; /* 提高层级确保按钮可点击 */
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
    will-change: transform, padding-right, opacity;
    align-items: center;
    transition: all 0.5s cubic-bezier(0.34, 1.56, 0.64, 1);
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
      transform: translateX(0);
      width: 40%;
      display: flex;
      flex-direction: column;
      align-items: flex-end;
      justify-content: center;
      transition: all 0.3s cubic-bezier(0.34, 1.56, 0.64, 1);
      padding-right: 5rem;
      box-sizing: border-box;
      will-change: transform, width, padding-right;
    }

    .right {
      transform: translateX(0);
      flex: 1;
      height: 100%;
      will-change: transform;

      .lrcShow {
        height: 100%;
        display: flex;
        justify-content: center;
        flex-direction: column;

        .data {
          padding: 0 3vh;
          margin-bottom: 8px;

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

            .vue-slider {
              margin: 0 10px;
              width: 100% !important;
              transform: translateY(-1px);
              cursor: pointer;


              :deep(.vue-slider-rail) {
                background-color: #ffffff20;
                border-radius: 25px;

                .vue-slider-process {
                  background-color: var(--main-cover-color) !important;
                  transition: width 0.1s ease;
                }

                .vue-slider-dot {
                  width: 12px !important;
                  height: 12px !important;
                  box-shadow: none;
                  transition: transform 0.2s cubic-bezier(0.34, 1.56, 0.64, 1);
                  will-change: transform;
                  
                  &:hover, &:active {
                    transform: scale(1.3);
                  }
                }

                .vue-slider-dot-handle-focus {
                  box-shadow: none;
                }

                .vue-slider-dot-tooltip-inner {
                  background-color: var(--main-cover-color) !important;
                  backdrop-filter: blur(2px);
                  border: none
                }

                .vue-slider-dot-handle {
                  background-color: var(--main-cover-color) !important
                }

                .vue-slider-dot-tooltip-text {
                  color: black;
                }
              }
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

// 为Apple Music风格添加自定义动画
@keyframes albumRotate {
  0% { transform: perspective(1000px) rotateY(0deg); }
  50% { transform: perspective(1000px) rotateY(-5deg); }
  100% { transform: perspective(1000px) rotateY(0deg); }
}

@keyframes coverShadowPulse {
  0% { box-shadow: 0 10px 30px rgba(0, 0, 0, 0.3); }
  50% { box-shadow: 0 15px 40px rgba(0, 0, 0, 0.4); }
  100% { box-shadow: 0 10px 30px rgba(0, 0, 0, 0.3); }
}

@keyframes textGlow {
  0% { text-shadow: 0 0 10px rgba(255, 255, 255, 0.2); }
  50% { text-shadow: 0 0 15px rgba(255, 255, 255, 0.4); }
  100% { text-shadow: 0 0 10px rgba(255, 255, 255, 0.2); }
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
