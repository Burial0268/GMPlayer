import { acceptHMRUpdate, defineStore } from "pinia";
import { ref } from "vue";
import getLanguageData from "@/utils/getLanguageData";
import {
  preprocessLyrics,
  type ParsedLrcLine,
  type ParsedYrcLine,
  type SongLyric,
} from "@/utils/LyricsProcessor";
import useSettingDataStore from "./settingData";

declare const $message: any;

function createEmptySongLyric(): SongLyric {
  return {
    lrc: [],
    yrc: [],
    lrcAMData: [],
    yrcAMData: [],
    hasTTML: false,
    ttml: [],
    hasLrcTran: false,
    hasLrcRoma: false,
    hasYrc: false,
    hasYrcTran: false,
    hasYrcRoma: false,
    formattedLrc: "",
    processedLyrics: [],
    settingsHash: "",
  } as SongLyric;
}

function createInitialSongLyric(): SongLyric {
  return {
    ...createEmptySongLyric(),
    hasLrcTran: true,
    hasLrcRoma: true,
    hasYrcTran: true,
    hasYrcRoma: true,
    settingsHash: "true-false-false",
  };
}

export const useMusicLyricStore = defineStore("musicLyric", () => {
  const songLyric = ref<SongLyric>(createInitialSongLyric());
  const playSongLyricIndex = ref(-1);

  function resetSongLyricState() {
    songLyric.value = createEmptySongLyric();
    playSongLyricIndex.value = -1;
  }

  function setPlaySongLyric(value: any) {
    if (!value) {
      console.log("该歌曲暂无歌词");
      songLyric.value = createEmptySongLyric();
      return;
    }

    try {
      if (!value.lrc || value.lrc.length === 0) {
        console.log("注意：歌词数据中缺少lrc数组，尝试从yrc创建");
        if (value.yrc && value.yrc.length > 0) {
          value.lrc = value.yrc.map((yrcLine: ParsedYrcLine) => ({
            time: yrcLine.time,
            content: yrcLine.TextContent,
          }));
          console.log("已从yrc数据创建lrc数组");
        } else {
          value.lrc = [
            { time: 0, content: "暂无歌词" },
            { time: 999, content: "No Lyrics Available" },
          ];
          console.log("创建了占位符lrc数组");
        }
      }

      if (!value.lrcAMData || value.lrcAMData.length === 0) {
        if (value.yrcAMData && value.yrcAMData.length > 0) {
          console.log("使用yrcAMData作为lrcAMData的备用");
          value.lrcAMData = [...value.yrcAMData];
        } else {
          console.log("创建基本的lrcAMData数组");
          value.lrcAMData = value.lrc.map((line: ParsedLrcLine) => ({
            startTime: line.time * 1000,
            endTime: (line.time + 5) * 1000,
            words: [
              {
                word: line.content,
                startTime: line.time * 1000,
                endTime: (line.time + 5) * 1000,
              },
            ],
            translatedLyric: "",
            romanLyric: "",
            isBG: false,
            isDuet: false,
          }));
        }
      }

      if (value.hasTTML === undefined) value.hasTTML = false;
      if (value.ttml === undefined) value.ttml = [];

      console.time("预处理歌词");
      const settings = useSettingDataStore();
      try {
        preprocessLyrics(value, {
          showYrc: settings.showYrc,
          showRoma: settings.showRoma,
          showTransl: settings.showTransl,
        });
        console.log("歌词数据预处理完成");
      } catch (error) {
        console.warn("歌词预处理出错，将使用原始数据:", error);
      }
      console.timeEnd("预处理歌词");

      songLyric.value = value;
      console.log("歌词数据已存储到store:", songLyric.value);
    } catch (error) {
      $message.error(getLanguageData("getLrcError"));
      console.error(getLanguageData("getLrcError"), error);
      songLyric.value = {
        ...createEmptySongLyric(),
        lrc: [
          { time: 0, content: "加载歌词时出错" },
          { time: 999, content: "Error loading lyrics" },
        ],
        lrcAMData: [
          {
            startTime: 0,
            endTime: 5000,
            words: [{ word: "加载歌词时出错", startTime: 0, endTime: 5000 }],
            translatedLyric: "",
            romanLyric: "",
            isBG: false,
            isDuet: false,
          },
        ],
      };
    }
  }

  function syncCurrentLyricIndex(displayCurrentTime: number) {
    const settings = useSettingDataStore();
    const lyrics =
      !songLyric.value.hasYrc || !settings.showYrc ? songLyric.value.lrc : songLyric.value.yrc;

    if (!lyrics?.length) {
      playSongLyricIndex.value = -1;
      return;
    }

    let currentIndex = playSongLyricIndex.value;
    const offsetTime = displayCurrentTime + (settings.lyricTimeOffset ?? 0) / 1000;
    if (currentIndex > 0 && lyrics[currentIndex]?.time > offsetTime) currentIndex = -1;
    while (currentIndex < lyrics.length - 1 && lyrics[currentIndex + 1].time <= offsetTime) {
      currentIndex++;
    }
    playSongLyricIndex.value = currentIndex;
  }

  return {
    songLyric,
    playSongLyricIndex,
    resetSongLyricState,
    setPlaySongLyric,
    syncCurrentLyricIndex,
  };
});

if (import.meta.hot) {
  import.meta.hot.accept(acceptHMRUpdate(useMusicLyricStore, import.meta.hot));
}

export default useMusicLyricStore;
