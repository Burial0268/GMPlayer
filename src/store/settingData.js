import { defineStore } from "pinia";
import { NIcon } from "naive-ui";
import { WbSunnyFilled, DarkModeFilled } from "@vicons/material";
import getLanguageData from "@/utils/getLanguageData";

const useSettingDataStore = defineStore("settingData", {
  state: () => {
    return {
      // 全局主题
      theme: "light",
      themeAuto: true,
      themeType: "red",
      themeData: {},
      // 搜索历史
      searchHistory: true,
      // 轮播图显示
      bannerShow: true,
      // 自动签到
      autoSignIn: true,
      // 列表点击方式
      listClickMode: "dblclick",
      // 是否使用 Lyric Atlas API
      useLyricAtlasAPI: false,
      // 播放器样式
      playerStyle: "cover",
      // 底栏歌词显示
      bottomLyricShow: true,
      // 是否显示逐字歌词
      showYrc: true,
      // 是否显示逐字歌词动画
      showYrcAnimation: true,
      // 是否显示逐字歌词上浮
      showYrcTransform: false,
      // 是否显示歌词翻译
      showTransl: false,
      // 是否显示歌词音译
      showRoma: false,
      // 歌曲音质
      songLevel: "exhigh",
      // 歌词位置
      lyricsPosition: "left",
      // 歌词滚动位置
      lyricsBlock: "top",
      // 歌词大小
      lyricsFontSize: 3.6,
      // 歌词字体
      lyricFont: 'HarmonyOS Sans SC',
      // 歌词字重
      lyricFontWeight: 'normal',
      // 歌词字间距
      lyricLetterSpacing: 'normal',
      // 歌词行高
      lyricLineHeight: 1.8,
      // 歌词模糊
      lyricsBlur: true,
      // 音乐频谱
      musicFrequency: false,
      // 鼠标移入歌词区域暂停滚动
      lrcMousePause: false,
      // 是否使用网易云解灰
      useUnmServer: true,
      // 播放器背景渲染模式
      backgroundImageShow: "eplor",
      // 模糊背景设置
      blurAmount: 10,
      contrastAmount: 1.2,
      // 流动背景设置
      fps: 60,
      flowSpeed: 2,
      renderScale: 0.5,
      albumImageUrl: "none",
      // 动态背景设置
      dynamicFlowSpeed: true,
      dynamicFlowSpeedScale: 1,
      // 是否显示前奏等待
      countDownShow: true,
      // 是否显示歌词设置
      showLyricSetting: false,
      // 歌曲渐入渐出
      songVolumeFade: true,
      // 列表默认数量
      listNumber: 30,
      // 记忆上次播放位置
      memoryLastPlaybackPosition: true,
      // 语言
      language: "zh-CN",
      // 底栏点击展开播放器
      bottomClick: false,
      // 沉浸式播放器
      immersivePlayer: false,
      // 沉浸式播放器取色类型
      colorType: "secondary",
      // 弹簧动画参数
      springParams: {
        posX: {
          mass: 1,
          damping: 10,
          stiffness: 100
        },
        posY: {
          mass: 1,
          damping: 15,
          stiffness: 100
        },
        scale: {
          mass: 1,
          damping: 20,
          stiffness: 100
        }
      }
    };
  },
  getters: {
    // 获取明暗模式
    getSiteTheme(state) {
      return state.theme;
    },
  },
  actions: {
    // 切换明暗模式
    setSiteTheme(value) {
      const isLightMode = value === "light";
      const message = isLightMode
        ? getLanguageData("lightMode")
        : getLanguageData("darkMode");
      const icon = isLightMode ? WbSunnyFilled : DarkModeFilled;
      this.theme = value;
      $message.info(message, {
        icon: () => h(NIcon, null, { default: () => h(icon) }),
      });
    },
    // 更改翻译开启选项
    setShowTransl(value) {
      this.showTransl = value;
    },
  },
  // 开启数据持久化
  persist: [
    {
      storage: localStorage,
    },
  ],
});

export default useSettingDataStore;
