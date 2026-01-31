import { defineStore } from "pinia";
import { nextTick } from "vue";
import { getSongTime, getSongPlayingTime } from "@/utils/timeTools";
import { getPersonalFm, setFmTrash } from "@/api/home";
import { getLikelist, setLikeSong } from "@/api/user";
import { getPlayListCatlist } from "@/api/playlist";
import { getMusicUrl } from "@/api/song";
import { userStore, settingStore } from "@/store";
import { NIcon } from "naive-ui";
import { PlayCycle, PlayOnce, ShuffleOne } from "@icon-park/vue-next";
import { soundStop, fadePlayOrPause } from "@/utils/AudioContext";
import getLanguageData from "@/utils/getLanguageData";
import { preprocessLyrics } from "@/utils/LyricsProcessor";

const useMusicDataStore = defineStore("musicData", {
  state: () => {
    return {
      // 是否展示播放界面
      showBigPlayer: false,
      // 是否展示播放控制条
      showPlayBar: true,
      // 是否展示播放列表
      showPlayList: false,
      // 播放状态
      playState: false,
      // 当前歌曲歌词数据
      songLyric: {
        hasLrcTran: true,
        hasLrcRoma: true,
        hasYrc: false,
        hasYrcTran: true,
        hasYrcRoma: true,
        hasTTML: false,
        lrc: [],
        yrc: [],
        ttml: [],
        lrcAMData: [],
        yrcAMData: [],
        formattedLrc: "[69:10.00]吹き込んだそよ風が\n[69:10.00]从窗外吹进的微风\n[69:10.00]fu ki ko n da so yo ka ze ga\n[120:20.00]窓辺の花を揺らして\n[120:20.00]让窗旁的花儿随风摇曳\n[120:20.00]ma do be no ha na wo yu ra shi te\n[192:30.00]浮かんだ面影と\n[192:30.00]浮现内心的过去身影\n[192:30.00]u ka n da o mo ka ge to\n[244:00.00]春を貪った日々のこと\n[244:00.00]与那盗春往日\n[244:00.00]ha ru wo mu sa bo tta hi bi no ko to\n[437:40.00]読みかけた本の中\n[437:40.00]那读至一半的书本\n[437:40.00]yo mi ka ke ta ho n no na ka\n[490:40.00]夢中になって追いかけて\n[490:40.00]我曾痴迷地追逐着其中文字\n[490:40.00]mu chu u ni na tte o i ka ke te\n[562:10.00]いつの日か忘れてた\n[562:10.00]如今也忘却是何时所读\n[562:10.00]i tsu no hi ka wa su re te ta\n[611:50.00]栞は挟んだままなのに\n[611:50.00]明明那书签未曾取出\n[611:50.00]shi o ri wa ha sa n da ma ma na no ni\n[674:10.00]君との日々もあてのない夢も\n[674:10.00]无论是曾与你度过的时光 还是那不知去向的梦想\n[674:10.00]ki mi to no hi bi mo a te no na i yu me mo\n[772:10.00]色褪せずに記憶の奥底で熱を放つ\n[772:10.00]都未曾褪色 依旧在我记忆深处着散发热度\n[772:10.00]i ro a se zu ni ki o ku no o ku so ko de ne tsu wo ha na tsu\n[970:50.00]描いた未来は遥か遠く離れても\n[970:50.00]纵使我们所描绘的未来是如此遥远\n[970:50.00]e ga i ta mi ra i wa ha ru ka to o ku ha na re te mo\n[1093:10.00]君といた光が明日を照らすから\n[1093:10.00]只要与你在一起 光芒便会照耀明日\n[1093:10.00]ki mi to i ta hi ka ri ga a shi ta wo te ra su ka ra\n[1225:20.00]風に乗せてこんな言葉も\n[1225:20.00]这句话语也乘风而去\n[1225:20.00]ka ze ni no se te ko n na ko to ba mo\n[1286:00.00]伝えられたのなら\n[1286:00.00]若能传达予你\n[1286:00.00]tsu ta e ra re ta no na ra\n[1346:00.00]それだけで僕はもう生きてゆける\n[1346:00.00]仅是如此 就能令我继续活下去\n[1346:00.00]so re da ke de bo ku wa mo u i ki te yu ke ru\n[1485:40.00]数え切れない程の喜怒哀楽を重ねた\n[1485:40.00]经历了数不清的喜怒哀乐\n[1485:40.00]ka zo e ki re na i ho do no ki do a i ra ku wo ka sa ne ta\n[1607:40.00]閉じた瞼にさえ愛しい時間が溢れる\n[1607:40.00]就连闭上双目 依旧会浮现那可爱的时光\n[1607:40.00]to ji ta ma bu ta ni sa e i to shi i ji ka n ga a fu re ru\n[1720:50.00]もう怖くないよ 迷いもしないよ\n[1720:50.00]我已不再畏惧 不再迷茫\n[1720:50.00]mo u ko wa ku na i yo ma yo i mo shi na i yo\n[1817:10.00]この思い出を\n[1817:10.00]这份回忆\n[1817:10.00]ko no o mo i de wo\n[1862:20.00]拾ってまた僕は歩き出せる\n[1862:20.00]我将其捡起又迈步向前\n[1862:20.00]hi ro tte ma ta bo ku wa a ru ki da se ru\n[2017:50.00]願っても縋っても叶わない運命でも\n[2017:50.00]纵使这份命运令祈祷无法实现\n[2017:50.00]ne ga tte mo su ga tte mo ka na wa na i u n me i de mo\n[2139:10.00]僕らはその先で待ち合わせをしよう\n[2139:10.00]就让我们在那前方相会吧\n[2139:10.00]bo ku ra wa so no sa ki de ma chi a wa se wo shi yo u\n[2270:40.00]どんな夜もきっと越えられる\n[2270:40.00]无论怎样的夜晚都能跨越\n[2270:40.00]do n na yo ru mo ki tto ko e ra re ru\n[2331:20.00]君との旅路なら\n[2331:20.00]若是与你一起的旅途\n[2331:20.00]ki mi to no ta bi ji na ra\n[2394:50.00]巡り合う奇跡を信じて欲しい\n[2394:50.00]我想坚信那相逢的奇迹\n[2394:50.00]me gu ri a u ki se ki wo shi n ji te ho shi i\n[2530:10.00]躓いても間違ってでも\n[2530:10.00]即便受尽挫折 屡屡犯错\n[2530:10.00]tsu ma zu i te mo ma chi ga tte de mo\n[2593:50.00]ここまで歩いて来たんだ\n[2593:50.00]我们也都走到这里了\n[2593:50.00]ko ko ma de a ru i te ki ta n da\n[2648:00.00]刻んだ足跡は\n[2648:00.00]铭刻下的足迹\n[2648:00.00]ki za n da a shi a to wa\n[2709:50.00]僕らだけのもの\n[2709:50.00]皆是仅属于我们的存在\n[2709:50.00]bo ku ra da ke no mo no\n[2786:10.00]交わした誓いも\n[2786:10.00]无论是互换的誓言\n[2786:10.00]ka wa shi ta chi ka i mo\n[2908:40.00]重ねたあの日々も\n[2908:40.00]还是一同度过的时光\n[2908:40.00]ka sa ne ta a no hi bi mo\n[3033:10.00]描いた未来は遥か遠く離れても\n[3033:10.00]纵使我们所描绘的未来是那么遥远\n[3033:10.00]e ga i ta mi ra i wa ha ru ka to o ku ha na re te mo\n[3155:10.00]君といた光が明日を照らすから\n[3155:10.00]只要与你在一起 光芒便会照耀明日\n[3155:10.00]ki mi to i ta hi ka ri ga a shi ta wo te ra su ka ra\n[3284:50.00]いつか向き合った夢の先\n[3284:50.00]过去所面向的梦想前方\n[3284:50.00]i tsu ka mu ki a tta yu me no sa ki\n[3346:20.00]すれ違った道でも\n[3346:20.00]还有那擦肩而过的道路\n[3346:20.00]su re chi ga tta mi chi de mo\n[3408:20.00]温もりを頼りに歩いてゆこう\n[3408:20.00]就依靠这份温暖迈步向前吧\n[3408:20.00]nu ku mo ri wo ta yo ri ni a ru i te yu ko u\n[3532:40.00]その先で必ず巡り合うから\n[3532:40.00]因为在那前方你我定会相遇\n[3532:40.00]so no sa ki de ka na ra zu me gu ri a u ka ra\n[3698:10.00]吹き込んだそよ風が\n[3698:10.00]从窗外吹进的微风\n[3698:10.00]fu ki ko n da so yo ka ze ga\n[3750:10.00]窓辺の花を揺らして\n[3750:10.00]让窗旁的花儿随风摇曳\n[3750:10.00]ma do be no ha na wo yu ra shi te\n[3824:00.00]仰ぐ今日の空は\n[3824:00.00]仰望今日的天空\n[3824:00.00]a o gu kyo u no so ra wa\n[3874:20.00]あの時描いた青だった\n[3874:20.00]是我那时所描绘的蔚蓝\n[3874:20.00]a no to ki e ga i ta a o da tta\n",
        processedLyrics: [],
        settingsHash: "true-false-false"
      },
      // 当前歌曲歌词播放索引
      playSongLyricIndex: -1,
      // 每日推荐
      dailySongsData: [],
      // 歌单分类
      catList: {},
      // 精品歌单分类
      highqualityCatList: [],
      // 音乐频谱数据
      spectrumsData: [],
      spectrumsScaleData: 1,
      // 低频音量 (用于背景动画)
      lowFreqVolume: 0,
      // 是否正在加载数据
      isLoadingSong: false,
      // 预加载过的歌曲ID
      preloadedSongIds: new Set(),
      // 持久化数据
      persistData: {
        // 搜索历史
        searchHistory: [],
        // 是否处于私人 FM 模式
        personalFmMode: false,
        // 私人 FM 数据
        personalFmData: {},
        // 播放列表类型
        playListMode: "list",
        // 喜欢音乐列表
        likeList: [],
        // 播放列表
        playlists: [],
        // 当前歌曲索引
        playSongIndex: 0,
        // 当前播放模式
        // normal-顺序播放 random-随机播放 single-单曲循环
        playSongMode: "normal",
        // 当前播放时间
        playSongTime: {
          currentTime: 0,
          duration: 0,
          barMoveDistance: 0,
          songTimePlayed: "00:00",
          songTimeDuration: "00:00",
        },
        // 播放音量
        playVolume: 0.7,
        // 静音前音量
        playVolumeMute: 0,
        // 列表状态
        playlistState: 0, // 0 顺序 1 单曲循环 2 随机
        // 播放历史
        playHistory: [],
      },
    };
  },
  getters: {
    // 获取是否处于私人FM模式
    getPersonalFmMode(state) {
      return state.persistData.personalFmMode;
    },
    // 获取私人FM模式数据
    getPersonalFmData(state) {
      return state.persistData.personalFmData;
    },
    // 获取是否正在加载数据
    getLoadingState(state) {
      return state.isLoadingSong;
    },
    // 获取每日推荐
    getDailySongs(state) {
      return state.dailySongsData;
    },
    // 获取播放列表
    getPlaylists(state) {
      return state.persistData.playlists;
    },
    // 获取频谱数据
    getSpectrumsData(state) {
      return state.spectrumsData
    },
    // 获取低频音量
    getLowFreqVolume(state) {
      return state.lowFreqVolume
    },
    // 获取播放模式
    getPlaySongMode(state) {
      return state.persistData.playSongMode;
    },
    // 获取当前歌曲
    getPlaySongData(state) {
      return state.persistData.playlists[state.persistData.playSongIndex];
    },
    // 获取当前歌词
    getPlaySongLyric(state) {
      return state.songLyric;
    },
    // 获取当前歌词索引
    getPlaySongLyricIndex(state) {
      return state.playSongLyricIndex;
    },
    // 获取当前播放时间
    getPlaySongTime(state) {
      return state.persistData.playSongTime;
    },
    // 获取播放状态
    getPlayState(state) {
      return state.playState;
    },
    // 获取喜欢音乐列表
    getLikeList(state) {
      return state.persistData.likeList;
    },
    // 获取播放历史
    getPlayHistory(state) {
      return state.persistData.playHistory;
    },
    // 获取播放列表模式
    getPlayListMode(state) {
      return state.persistData.playListMode;
    },
    // 获取搜索历史
    getSearchHistory(state) {
      return state.persistData.searchHistory;
    },
  },
  actions: {
    // 预加载接下来 5 首歌曲
    preloadUpcomingSongs() {
      // 防御式检查：确保 preloadedSongIds 是一个 Set
      if (!(this.preloadedSongIds instanceof Set)) {
        console.warn(
          "preloadedSongIds 类型不正确，已重置。这可能在页面刷新后发生。"
        );
        this.preloadedSongIds = new Set();
      }
      // 必须是非私人 FM 模式
      if (this.persistData.personalFmMode) {
        console.log("预加载已跳过：私人 FM 模式");
        return;
      }
      const playlist = this.persistData.playlists;
      const listLength = playlist.length;
      // 列表歌曲数小于2，或不是顺序播放模式，则不预加载
      if (listLength < 2 || this.persistData.playSongMode !== "normal") {
        console.log(
          `预加载已跳过：歌曲数 ${listLength} / 播放模式 ${this.persistData.playSongMode}`
        );
        return;
      }

      const currentIndex = this.persistData.playSongIndex;
      const preloadCount = 5;
      const songsToPreload = [];

      for (let i = 0; i <= preloadCount; i++) {
        const nextIndex = (currentIndex + i) % listLength;
        const songData = playlist[nextIndex];
        // 避免重复预加载
        if (songData && !this.preloadedSongIds.has(songData.id)) {
          songsToPreload.push(songData);
        }
      }

      if (!songsToPreload.length) {
        console.log("没有需要预加载的新歌曲");
        return;
      }

      console.log(
        "即将并行预加载歌曲:",
        songsToPreload.map((s) => s.name).join(", ")
      );

      const urlPromises = songsToPreload.map((songData) =>
        getMusicUrl(songData.id)
          .then((res) => {
            if (res.data[0]?.url) {
              return {
                id: songData.id,
                name: songData.name,
                url: res.data[0].url.replace(/^http:/, "https:"),
              };
            }
            return null;
          })
          .catch((err) => {
            console.error(`获取 ${songData.name} URL 失败`, err);
            return null;
          })
      );

      Promise.all(urlPromises).then((results) => {
        const validSongs = results.filter(Boolean);
        if (!validSongs.length) return;

        const fetchPromises = validSongs.map((song) =>
          fetch(song.url)
            .then((response) => {
              if (response.ok) {
                console.log(`歌曲 ${song.name} 预加载完成`);
                this.preloadedSongIds.add(song.id);
              } else {
                throw new Error(`Response status: ${response.status}`);
              }
            })
            .catch((err) => {
              console.warn(`歌曲 ${song.name} 预加载请求失败`, err);
            })
        );

        Promise.all(fetchPromises).then(() => {
          console.log("本批次预加载任务全部结束");
        });
      });
    },
    // 更改是否处于私人FM模式
    setPersonalFmMode(value) {
      this.persistData.personalFmMode = value;
      if (value) {
        if (typeof $player !== "undefined") soundStop($player);
        if (this.persistData.personalFmData?.id) {
          this.persistData.playlists = [];
          this.persistData.playlists.push(this.persistData.personalFmData);
          this.persistData.playSongIndex = 0;
        } else {
          this.setPersonalFmData();
        }
      }
    },
    // 当处于私人fm模式时更改歌单
    setPersonalFmData() {
      try {
        const songName = this.getPersonalFmData?.name;
        getPersonalFm().then((res) => {
          if (res.data[0]) {
            const data = res.data[2] || res.data[0];
            const fmData = {
              id: data.id,
              name: data.name,
              artist: data.artists,
              album: data.album,
              alia: data.alias,
              time: getSongTime(data.duration),
              fee: data.fee,
              pc: data.pc ? data.pc : null,
              mv: data.mvid,
            };
            if (songName && songName == fmData.name) {
              this.setFmDislike(fmData.id, false);
            } else {
              this.persistData.personalFmData = fmData;
              if (this.persistData.personalFmMode) {
                if (typeof $player !== "undefined") soundStop($player);
                this.persistData.playlists = [];
                this.persistData.playlists.push(fmData);
                this.persistData.playSongIndex = 0;
                this.setPlayState(true);
              }
            }
          } else {
            $message.error(getLanguageData("personalFmError"));
          }
        });
      } catch (err) {
        console.error(getLanguageData("personalFmError"), err);
        $message.error(getLanguageData("personalFmError"));
      }
    },
    // 私人fm垃圾桶
    setFmDislike(id) {
      const user = userStore();
      if (user.userLogin) {
        setFmTrash(id).then((res) => {
          if (res.code == 200) {
            this.persistData.personalFmMode = true;
            this.setPlaySongIndex("next");
          } else {
            $message.error(getLanguageData("fmTrashError"));
          }
        });
      } else {
        $message.error(getLanguageData("needLogin"));
      }
    },
    // 更改喜欢列表
    setLikeList() {
      const user = userStore();
      if (user.userLogin) {
        getLikelist(user.getUserData.id).then((res) => {
          this.persistData.likeList = res.ids;
        });
      }
    },
    // 查找歌曲是否处于喜欢列表
    getSongIsLike(id) {
      return this.persistData.likeList.includes(id);
    },
    // 移入移除喜欢列表
    async changeLikeList(id, like = true) {
      const user = userStore();
      const list = this.persistData.likeList;
      const exists = list.includes(id);
      if (!user.userLogin) {
        $message.error(getLanguageData("needLogin"));
        return;
      }
      try {
        const res = await setLikeSong(id, like);
        if (res.code === 200) {
          if (like && !exists) {
            list.push(id);
            $message.info(getLanguageData("loveSong"));
          } else if (!like && exists) {
            list.splice(list.indexOf(id), 1);
            $message.info(getLanguageData("loveSongRemove"));
          } else if (like && exists) {
            $message.info(getLanguageData("loveSongRepeat"));
          }
        } else {
          if (like) {
            $message.error(getLanguageData("loveSongError"));
          } else {
            $message.error(getLanguageData("loveSongRemoveError"));
          }
        }
      } catch (error) {
        console.error(getLanguageData("loveSongError"), error);
        $message.error(getLanguageData("loveSongError"));
      }
    },
    // 更改音乐播放状态
    setPlayState(value) {
      this.playState = value;
    },
    // 更改展示播放界面
    setBigPlayerState(value) {
      this.showBigPlayer = value;
    },
    // 更改播放条状态
    setPlayBarState(value) {
      this.showPlayBar = value;
    },
    // 更改播放列表模式
    setPlayListMode(value) {
      this.persistData.playListMode = value;
    },
    // 添加歌单至播放列表
    setPlaylists(value) {
      this.persistData.playlists = value.slice();
      this.preloadedSongIds.clear();
    },
    // 更改每日推荐数据
    setDailySongs(value) {
      if (value) {
        this.dailySongsData = [];
        value.forEach((v) => {
          this.dailySongsData.push({
            id: v.id,
            name: v.name,
            artist: v.ar,
            album: v.al,
            alia: v.alia,
            time: getSongTime(v.dt),
            fee: v.fee,
            pc: v.pc ? v.pc : null,
            mv: v.mv ? v.mv : null,
          });
        });
      }
    },
    // 歌词处理
    setPlaySongLyric(value) {
      if (value) {
        try {
          // 确保歌词数据中始终有lrc歌词数组
          if (!value.lrc || value.lrc.length === 0) {
            console.log("注意：歌词数据中缺少lrc数组，尝试从yrc创建");
            
            // 如果有yrc数据但没有lrc数据，尝试从yrc创建lrc
            if (value.yrc && value.yrc.length > 0) {
              value.lrc = value.yrc.map(yrcLine => ({
                time: yrcLine.time,
                content: yrcLine.TextContent
              }));
              console.log("已从yrc数据创建lrc数组");
            } else {
              // 如果没有yrc数据，创建占位符lrc
              value.lrc = [
                { time: 0, content: "暂无歌词" },
                { time: 999, content: "No Lyrics Available" }
              ];
              console.log("创建了占位符lrc数组");
            }
          }
          
          // 确保lrcAMData存在
          if (!value.lrcAMData || value.lrcAMData.length === 0) {
            if (value.yrcAMData && value.yrcAMData.length > 0) {
              // 如果有yrcAMData但没有lrcAMData，使用yrcAMData作为备用
              console.log("使用yrcAMData作为lrcAMData的备用");
              value.lrcAMData = [...value.yrcAMData];
            } else {
              // 创建基本的lrcAMData
              console.log("创建基本的lrcAMData数组");
              value.lrcAMData = value.lrc.map(line => ({
                startTime: line.time * 1000,
                endTime: (line.time + 5) * 1000, // 假设每行持续5秒
                words: [{
                  word: line.content,
                  startTime: line.time * 1000,
                  endTime: (line.time + 5) * 1000
                }],
                translatedLyric: "",
                romanLyric: "",
                isBG: false,
                isDuet: false
              }));
            }
          }
          
          // 确保TTML相关字段存在
          if (value.hasTTML === undefined) {
            value.hasTTML = false;
          }
          if (value.ttml === undefined) {
            value.ttml = [];
          }
          
          // 在存入状态前预处理歌词数据，提高性能
          console.time('预处理歌词');
          const settings = settingStore();
          try {
            // 预处理并缓存处理后的结果
            preprocessLyrics(value, {
              showYrc: settings.showYrc,
              showRoma: settings.showRoma,
              showTransl: settings.showTransl
            });
            console.log("歌词数据预处理完成");
          } catch (err) {
            console.warn("歌词预处理出错，将使用原始数据:", err);
          }
          console.timeEnd('预处理歌词');
          
          this.songLyric = value;
          console.log("歌词数据已存储到store:", this.songLyric);
        } catch (err) {
          $message.error(getLanguageData("getLrcError"));
          console.error(getLanguageData("getLrcError"), err);
          
          // 即使出错，也确保有基本的歌词结构
          this.songLyric = {
            lrc: [
              { time: 0, content: "加载歌词时出错" },
              { time: 999, content: "Error loading lyrics" }
            ],
            yrc: [],
            lrcAMData: [{
              startTime: 0,
              endTime: 5000,
              words: [{
                word: "加载歌词时出错",
                startTime: 0,
                endTime: 5000
              }],
              translatedLyric: "",
              romanLyric: "",
              isBG: false,
              isDuet: false
            }],
            yrcAMData: [],
            hasTTML: false,  // 出错时也设置TTML相关字段
            ttml: [],
            hasLrcTran: false,
            hasLrcRoma: false,
            hasYrc: false,
            hasYrcTran: false,
            hasYrcRoma: false,
            formattedLrc: ""
          };
        }
      } else {
        console.log("该歌曲暂无歌词");
        this.songLyric = {
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
          formattedLrc: ""
        };
      }
    },
    // 歌曲播放进度
    setPlaySongTime(value) {
      this.persistData.playSongTime.currentTime = value.currentTime;
      this.persistData.playSongTime.duration = value.duration;
      // 计算进度条应该移动的距离
      if (value.duration === 0) {
        this.persistData.playSongTime.barMoveDistance = 0;
      } else {
        this.persistData.playSongTime.barMoveDistance =
          (value.currentTime / value.duration) * 100;
      }

      if (!Number.isNaN(this.persistData.playSongTime.barMoveDistance)) {
        // 歌曲播放进度转换
        this.persistData.playSongTime.songTimePlayed = getSongPlayingTime(
          (value.duration / 100) * this.persistData.playSongTime.barMoveDistance
        );
        this.persistData.playSongTime.songTimeDuration = getSongPlayingTime(
          value.duration
        );
      }
      // 计算当前歌词播放索引
      const setting = settingStore();
      const lrcType = !this.songLyric.hasYrc || !setting.showYrc;
      const lyrics = lrcType ? this.songLyric.lrc : this.songLyric.yrc;

      // 优化歌词索引查找
      if (!lyrics || !lyrics.length) {
        this.playSongLyricIndex = -1;
        return;
      }

      let currentIndex = this.playSongLyricIndex;

      // 当进度条回溯时，重置索引从头开始查找
      if (currentIndex > 0 && lyrics[currentIndex]?.time > value.currentTime) {
        currentIndex = -1;
      }

      // 从当前索引向后查找，直到找到第一个时间大于当前播放时间的歌词
      while (
        currentIndex < lyrics.length - 1 &&
        lyrics[currentIndex + 1].time <= value.currentTime
      ) {
        currentIndex++;
      }

      this.playSongLyricIndex = currentIndex;
    },
    // 设置当前播放模式
    setPlaySongMode(value = null) {
      const modeObj = {
        normal: PlayCycle,
        random: ShuffleOne,
        single: PlayOnce,
      };
      if (value && value in modeObj) {
        this.persistData.playSongMode = value;
      } else {
        switch (this.persistData.playSongMode) {
          case "normal":
            this.persistData.playSongMode = "random";
            value = "random";
            break;
          case "random":
            this.persistData.playSongMode = "single";
            value = "single";
            break;
          default:
            this.persistData.playSongMode = "normal";
            value = "normal";
            break;
        }
      }
      $message.info(getLanguageData(value), {
        icon: () =>
          h(NIcon, null, {
            default: () => h(modeObj[this.persistData.playSongMode]),
          }),
      });
    },
    // 上下曲调整
    setPlaySongIndex(type) {
      // 如果 $player 未定义，返回 false
      if (typeof $player === "undefined") return false;
      // 停止播放当前歌曲
      soundStop($player)
      // 根据播放模式设置加载状态
      if (this.persistData.playSongMode !== "single") {
        this.isLoadingSong = true;
      }
      // 如果是个人 FM 模式，设置个人 FM 数据
      if (this.persistData.personalFmMode) {
        this.setPersonalFmData();
      } else {
        const listLength = this.persistData.playlists.length;
        const listMode = this.persistData.playSongMode;
        // 根据当前播放模式调整播放索引
        if (listMode === "normal") {
          this.persistData.playSongIndex += type === "next" ? 1 : -1;
        } else if (listMode === "random") {
          this.persistData.playSongIndex = Math.floor(
            Math.random() * listLength
          );
        } else if (listMode === "single") {
          // 单曲循环模式
          console.log("单曲循环模式");
          fadePlayOrPause($player, "play", this.persistData.playVolume);
        } else {
          // 未知播放模式，显示错误消息
          $message.error(getLanguageData("playError"));
        }
        // 检查播放索引是否越界，并根据情况进行处理
        if (listMode !== "single") {
          if (this.persistData.playSongIndex < 0) {
            this.persistData.playSongIndex = listLength - 1;
          } else if (this.persistData.playSongIndex >= listLength) {
            this.persistData.playSongIndex = 0;
            soundStop($player);
            fadePlayOrPause($player, "play", this.persistData.playVolume);
          }
          // 如果播放列表长度大于 1，则停止播放当前歌曲
          if (listLength > 1) {
            soundStop($player);
          }
          // 在下一个事件循环中设置播放状态
          nextTick().then(() => {
            this.setPlayState(true);
          });
        }
      }
    },
    // 添加歌曲至播放列表
    addSongToPlaylists(value, play = true) {
      // 停止当前播放
      if (typeof $player !== "undefined") soundStop($player);
      // 判断与上一次播放歌曲是否一致
      const index = this.persistData.playlists.findIndex(
        (o) => o.id === value.id
      );
      try {
        if (
          value.id !==
          this.persistData.playlists[this.persistData.playSongIndex]?.id
        ) {
          console.log("Play a song that is not the same as the last one");
          if (typeof $player !== "undefined") soundStop($player);
          this.isLoadingSong = true;
        }
      } catch (error) {
        console.error("Error:" + error);
      }
      if (index !== -1) {
        this.persistData.playSongIndex = index;
      } else {
        this.persistData.playlists.push(value);
        this.persistData.playSongIndex = this.persistData.playlists.length - 1;
      }
      play ? this.setPlayState(true) : null;
    },
    // 在当前播放歌曲后添加
    addSongToNext(value) {
      // 更改播放模式为列表循环
      this.persistData.playSongMode = "normal";
      // 查找是否存在于播放列表
      const index = this.persistData.playlists.findIndex(
        (o) => o.id === value.id
      );
      if (index !== -1) {
        console.log(index);
        if (index === this.persistData.playSongIndex) return true;
        if (index < this.persistData.playSongIndex)
          this.persistData.playSongIndex--;
        const arr = this.persistData.playlists.splice(index, 1)[0];
        this.persistData.playlists.splice(
          this.persistData.playSongIndex + 1,
          0,
          arr
        );
      } else {
        this.persistData.playlists.splice(
          this.persistData.playSongIndex + 1,
          0,
          value
        );
      }
      $message.success(value.name + " " + getLanguageData("addSongToNext"));
    },
    // 播放列表移除歌曲
    removeSong(index) {
      if (typeof $player === "undefined") return false;
      const songId = this.persistData.playlists[index].id;
      const name = this.persistData.playlists[index].name;
      if (index < this.persistData.playSongIndex) {
        this.persistData.playSongIndex--;
      } else if (index === this.persistData.playSongIndex) {
        // 如果删除的是当前播放歌曲，则重置播放器
        soundStop($player);
      }
      $message.success(name + " " + getLanguageData("removeSong"));
      this.persistData.playlists.splice(index, 1);
      this.preloadedSongIds.delete(songId);
      // 检查当前播放歌曲的索引是否超出了列表范围
      if (this.persistData.playSongIndex >= this.persistData.playlists.length) {
        this.persistData.playSongIndex = 0;
        soundStop($player);
      }
    },
    // 获取歌单分类
    setCatList(highquality = false) {
      getPlayListCatlist().then((res) => {
        if (res.code == 200) {
          this.catList = res;
        } else {
          $message.error(getLanguageData("getDataError"));
        }
      });
      if (highquality) {
        getPlayListCatlist(true).then((res) => {
          if (res.code == 200) {
            this.highqualityCatList = res.tags;
          } else {
            $message.error(getLanguageData("getDataError"));
          }
        });
      }
    },
    // 更改播放历史
    setPlayHistory(data, clean = false) {
      if (clean) {
        this.persistData.playHistory = [];
      } else {
        const index = this.persistData.playHistory.findIndex(
          (item) => item.id === data.id
        );
        if (index !== -1) {
          this.persistData.playHistory.splice(index, 1);
          // return false;
        }
        if (this.persistData.playHistory.length > 100)
          this.persistData.playHistory.pop();
        this.persistData.playHistory.unshift(data);
      }
    },
    // 更改搜索历史
    setSearchHistory(name, clean = false) {
      if (clean) {
        this.persistData.searchHistory = [];
      } else {
        const index = this.persistData.searchHistory.indexOf(name);
        if (index !== -1) {
          this.persistData.searchHistory.splice(index, 1);
        }
        this.persistData.searchHistory.unshift(name);
        if (this.persistData.searchHistory.length > 30) {
          this.persistData.searchHistory.pop();
        }
      }
    },
    // 更新当前播放时间
    updateCurrentTime(time) {
      this.currentTime = Math.floor(time * 1000); // 转换为毫秒
    },
    // 设置加载状态
    setLoadingState(state) {
      this.isLoadingSong = state;
    },
    // 设置播放状态
    setPlayState(state) {
      this.playState = state;
    },
  },
  // 开启数据持久化
  persist: [
    {
      storage: localStorage,
      paths: ["persistData"],
      afterRestore: (ctx) => {
        // 在状态恢复后，确保 preloadedSongIds 是一个 Set
        ctx.store.preloadedSongIds = new Set();
      },
    },
  ],
});

export default useMusicDataStore;
