import { defineStore, acceptHMRUpdate } from "pinia";
import {
  listenTogether,
  type RoomInfo,
  type RoomUser,
  type PlayStatus,
  type RoomStatus,
} from "@/api/listenTogether";
import { getMusicDetail } from "@/api/song";
import { useMusicDataStore, useUserDataStore } from "@/store";
import type { SongData } from "@/store/musicTypes";
// 直接引模块而非 barrel：NativeManifestPublisher 会读本 store 判断房间状态，
// 走 barrel 会把这个环放大到整个 AudioContext。两侧都只在运行时互相调用。
import { publishNativeManifest } from "@/utils/AudioContext/NativeManifestPublisher";
import { setNativeListenTogetherRoom } from "@/utils/AudioContext/NativeListenTogetherSync";
import { asRawEntry } from "@/utils/rawEntry";
import getLanguageData from "@/utils/getLanguageData";

declare const $message: any;
declare const $player: any;

/**
 * 房间角色类型
 */
export type RoomRole = "host" | "guest" | "none";

/**
 * 同步状态
 */
export type SyncStatus = "idle" | "syncing" | "error";

/**
 * 「重载后恢复房间」的时效上界。
 *
 * WebView 被销毁重建只隔几秒；超过这个窗口说明是一次真正的冷启动（应用被关掉
 * 过），此时静默重新入房是意外行为，宁可让用户重新点一次。
 */
const ROOM_RESUME_MAX_AGE_MS = 5 * 60 * 1000;

/**
 * 每隔几次状态轮询（3s 一次）顺带拉一次共享列表。
 *
 * `RoomStatus` 不带列表版本号，所以「对方加了歌但还没播」没有别的观测途径。
 * 5 次 ≈ 15s，是「即时感」与请求量之间的折中；`fetchRoomPlaylist` 会先比签名，
 * 列表没变时不会去取曲目详情。
 */
const PLAYLIST_POLL_EVERY_N_STATUS = 5;

/**
 * 一起听歌状态
 */
export interface ListenTogetherState {
  /** 是否启用一起听歌 */
  enabled: boolean;
  /** 当前房间信息 */
  roomInfo: RoomInfo | null;
  /** 用户角色 */
  role: RoomRole;
  /** 房间中的用户列表（不含房主自身） */
  users: RoomUser[];
  /** 客户端序列号（用于同步） */
  clientSeq: number;
  /** 同步状态 */
  syncStatus: SyncStatus;
  /** 轮询定时器 */
  pollTimer: number | null;
  /** 心跳定时器 */
  heartbeatTimer: number | null;
  /** 最后同步的歌曲ID */
  lastSyncedSongId: number | null;
  /** 最后同步的播放进度 */
  lastSyncedProgress: number;
  /** 最后同步的播放状态 */
  lastSyncedStatus: PlayStatus;
  /** 是否正在处理远程命令 */
  isProcessingRemoteCommand: boolean;
  /**
   * 最近一次「与房间达成一致」的列表签名（逗号拼接的 id）。
   *
   * 采纳远端列表和成功上报后都会更新。上报前拿它比一次，就不会把刚采纳的列表
   * 原样推回去 —— 靠 `isProcessingRemoteCommand` 那种时序守卫在这里不成立：
   * watcher 是 `flush: 'pre'`（微任务），回调跑的时候标志早就清掉了。
   * 不持久化：它只是会话内的去重，重载后重新对齐一次是正确的。
   */
  lastPlaylistSignature: string;
  /** 距离下一次拉取共享列表还差几次状态轮询。 */
  playlistPollTicks: number;
  /**
   * 最后一次确认房间还活着的时间戳（心跳成功时刷新）。
   *
   * 用来给「重载后恢复」加一个上界：WebView 重建只会隔几秒，而关掉应用几天后
   * 再开不该静默重新入房。故意不用 sessionStorage —— 它绑定顶层浏览上下文，
   * Activity 重建后就是全新上下文、内容为空，恰好在最需要恢复的场景下失效。
   */
  lastAliveAt: number;
}

/**
 * 一起听歌 Store
 */
const useListenTogetherStore = defineStore("listenTogether", {
  state: (): ListenTogetherState => ({
    enabled: false,
    roomInfo: null,
    role: "none",
    users: [],
    clientSeq: 0,
    syncStatus: "idle",
    pollTimer: null,
    heartbeatTimer: null,
    lastSyncedSongId: null,
    lastSyncedProgress: 0,
    lastSyncedStatus: "STOP",
    isProcessingRemoteCommand: false,
    lastPlaylistSignature: "",
    playlistPollTicks: 0,
    lastAliveAt: 0,
  }),

  getters: {
    /**
     * 是否处于房间中
     */
    isInRoom: (state): boolean => {
      return state.enabled && state.roomInfo !== null;
    },

    /**
     * 是否是房主
     */
    isHost: (state): boolean => {
      return state.role === "host";
    },

    /**
     * 是否是房客
     */
    isGuest: (state): boolean => {
      return state.role === "guest";
    },

    /**
     * 房间ID
     */
    roomId: (state): string | null => {
      return state.roomInfo?.roomId || null;
    },

    /**
     * 房主信息
     * RoomInfo 不再有 hostXxx 字段，改为从 roomUsers 中按 creatorId 匹配
     */
    hostInfo: (state): RoomUser | null => {
      if (!state.roomInfo) return null;
      return state.roomInfo.roomUsers.find((u) => u.userId === state.roomInfo!.creatorId) ?? null;
    },

    /**
     * 在线用户数量
     */
    onlineCount: (state): number => {
      return state.users.length + (state.role === "host" ? 1 : 0);
    },

    /**
     * 分享链接
     */
    shareLink: (state): string => {
      if (!state.roomInfo) return "";
      const musicData = useMusicDataStore();
      return `https://st.music.163.com/listen-together/share/index.html?roomId=${state.roomInfo.roomId}&inviterId=${state.roomInfo.creatorId}&songId=${musicData.getPlaySongData.id}`;
    },
  },

  actions: {
    /**
     * 创建房间（房主）
     */
    async createRoom(): Promise<boolean> {
      const musicStore = useMusicDataStore();
      const userStore = useUserDataStore();

      if (!userStore.userLogin) {
        $message.error(getLanguageData("needLogin"));
        return false;
      }

      const currentSong = musicStore.getPlaySongData;
      if (!currentSong?.id) {
        $message.error(getLanguageData("noSong"));
        return false;
      }

      try {
        const res = await listenTogether.createRoom();
        if (res.code === 200 && res.data) {
          // res.data.roomInfo 是完整的 RoomInfo，含 creatorId 和 roomUsers
          this.roomInfo = res.data.roomInfo;
          this.role = "host";
          this.enabled = true;
          this.clientSeq = 0;
          this.lastSyncedSongId = currentSong.id;
          // 初始化在线用户列表（排除房主自身）
          this.users = res.data.roomInfo.roomUsers.filter(
            (u) => u.userId !== res.data!.roomInfo.creatorId,
          );

          // 创建后立即 checkRoom，与参考实现保持一致
          await listenTogether.checkRoom(this.roomInfo.roomId);

          await this.syncCurrentPlaylist();
          // 发送初始播放命令，让服务端知道当前播放状态
          await this.sendPlayCommand("PLAY");
          // startHeartbeat 会立即发送一次带完整播放信息的心跳，
          // 服务端据此将房间从 NOT_CONNECTED 转为 CONNECTED
          this.startHeartbeat();
          this.startPolling();

          $message.success(getLanguageData("ltCreateSuccess"));
          return true;
        } else {
          $message.error(res.message || getLanguageData("ltCreateFailed"));
          return false;
        }
      } catch (error) {
        console.error("Create room failed:", error);
        $message.error(getLanguageData("ltCreateFailed"));
        return false;
      }
    },

    /**
     * 加入房间（房客）
     */
    async joinRoom(roomId: string): Promise<boolean> {
      const userStore = useUserDataStore();

      if (!userStore.userLogin) {
        $message.error(getLanguageData("needLogin"));
        return false;
      }

      try {
        // 参考实现流程：先 accept（joinRoom），再 checkRoom，再 getPlaylist
        const res = await listenTogether.joinRoom(roomId);
        if (res.code === 200 && res.data) {
          // joinRoom 响应中暂无完整 roomUsers，构造最小 RoomInfo
          this.roomInfo = {
            roomId: res.data.roomId,
            creatorId: res.data.hostUserId,
            effectiveDurationMs: 0,
            waitMs: 0,
            roomCreateTime: 0,
            chatRoomId: "",
            agoraChannelId: "",
            roomUsers: [],
            roomRTCType: null,
            roomType: "",
            ltType: 0,
            openHeartRcmd: false,
          };
          this.role = "guest";
          this.enabled = true;
          this.clientSeq = 0;

          // accept 成功后 checkRoom，获取房间完整信息
          await listenTogether.checkRoom(roomId);

          await this.fetchRoomPlaylist();
          this.startHeartbeat();
          this.startPolling();

          $message.success(getLanguageData("ltJoinSuccess"));
          return true;
        } else {
          $message.error(res.message || getLanguageData("ltJoinFailed"));
          return false;
        }
      } catch (error) {
        console.error("Join room failed:", error);
        $message.error(getLanguageData("ltJoinFailed"));
        return false;
      }
    },

    /**
     * 退出/关闭房间
     */
    async leaveRoom(): Promise<void> {
      if (!this.roomInfo) return;

      try {
        this.stopPolling();
        this.stopHeartbeat();

        if (this.role === "host") {
          const endRes = await listenTogether.endRoom(this.roomInfo.roomId);
          if (endRes.data && !endRes.data.success) {
            console.warn("End room returned success=false");
          }
        } else {
          await listenTogether.leaveRoom(this.roomInfo.roomId);
        }

        $message.success(
          this.role === "host"
            ? getLanguageData("ltCloseSuccess")
            : getLanguageData("ltLeaveSuccess"),
        );
      } catch (error) {
        console.error("Leave room error:", error);
      } finally {
        this.resetState();
      }
    },

    /**
     * 发送播放命令（房主或房客均可）
     */
    async sendPlayCommand(
      commandType: "PLAY" | "PAUSE" | "GOTO" | "seek",
      progress?: number,
    ): Promise<boolean> {
      if (!this.isInRoom || !this.roomInfo) return false;

      const musicStore = useMusicDataStore();
      const currentSong = musicStore.getPlaySongData;
      if (!currentSong?.id) return false;

      try {
        const currentProgress =
          progress ?? Math.floor(musicStore.getPlaySongPlaybackCurrentTime() * 1000);

        // playStatus 逻辑：PLAY/PAUSE 直接用命令本身，GOTO/seek 保持当前实际播放状态
        let playStatus: "PLAY" | "PAUSE";
        if (commandType === "PLAY") {
          playStatus = "PLAY";
        } else if (commandType === "PAUSE") {
          playStatus = "PAUSE";
        } else {
          // GOTO（切歌）和 seek（进度跳转）保持当前实际播放状态
          playStatus = musicStore.getPlayState ? "PLAY" : "PAUSE";
        }

        // formerSongId：GOTO（切歌）时传上一首歌的 ID，其他命令传 "-1"
        const formerSongId =
          commandType === "GOTO" && this.lastSyncedSongId ? this.lastSyncedSongId.toString() : "-1";

        const res = await listenTogether.sendPlayCommand({
          roomId: this.roomInfo.roomId,
          progress: currentProgress,
          commandType,
          formerSongId,
          targetSongId: currentSong.id,
          clientSeq: this.clientSeq++,
          playStatus,
        });

        if (res.code === 200) {
          this.lastSyncedSongId = currentSong.id;
          this.lastSyncedProgress = currentProgress;
          this.lastSyncedStatus = playStatus;
          // GOTO（切歌）时立即发送心跳，告知服务端新曲目信息
          if (commandType === "GOTO") {
            this.sendHeartbeat();
          }
          return true;
        }
        return false;
      } catch (error) {
        console.error("Send play command failed:", error);
        return false;
      }
    },

    /**
     * 把本地播放列表推给房间。
     *
     * 房主房客都可以 —— 一起听是「一份列表，两个消费者」，任何一方加歌两边都加。
     * （此前这里有 `if (!this.isHost) return`，房客加的歌根本传不出去。）
     *
     * 用 `REPLACE` 而不是 `ADD`：上游 API 的 `ADD` 到底期望「新增的 id」还是
     * 「完整列表」无从确认，猜错会污染共享列表；而在「共享同一份列表」的语义下，
     * 整表上报本来就是正确的，代价只是多传几 KB id。
     */
    async syncCurrentPlaylist(): Promise<boolean> {
      if (!this.isInRoom || !this.roomInfo) return false;

      const musicStore = useMusicDataStore();
      const userStore = useUserDataStore();
      const playlist = musicStore.getPlaylists;
      if (!playlist.length) return false;

      const trackIds = playlist.map((song) => song.id).join(",");
      // 已经和房间一致（刚采纳过，或刚推过）——别把同一份列表推回去。
      if (trackIds === this.lastPlaylistSignature) return false;

      try {
        const res = await listenTogether.syncPlaylist({
          roomId: this.roomInfo.roomId,
          commandType: "REPLACE",
          userId: userStore.userData.userId || 0,
          version: this.clientSeq++,
          playMode: musicStore.getPlaySongMode.toUpperCase(),
          displayList: trackIds,
          randomList: trackIds,
        });

        if (res.code === 200) {
          this.lastPlaylistSignature = trackIds;
          return true;
        }
        return false;
      } catch (error) {
        console.error("Sync playlist failed:", error);
        return false;
      }
    },

    /**
     * 拉取房间的共享播放列表并采纳。
     *
     * 曲目详情按 100 一批取 —— 之前是 `slice(0, 100)` 直接截断，超过 100 首的
     * 房间列表会静默丢掉后面的。
     */
    async fetchRoomPlaylist(): Promise<boolean> {
      if (!this.roomId) return false;

      try {
        const res = await listenTogether.getPlaylist(this.roomId);
        const songIds = res.data?.playlist?.displayList?.result;
        if (res.code !== 200 || !songIds?.length) return false;

        // 先比签名再取详情：周期轮询下绝大多数情况列表没变，`getPlaylist` 只回
        // id 列表（便宜），而 `getMusicDetail` 要按 100 分批打好几个请求（不便宜）。
        const signature = songIds.join(",");
        if (signature === this.lastPlaylistSignature) return true;

        const CHUNK = 100;
        const songs: SongData[] = [];
        for (let offset = 0; offset < songIds.length; offset += CHUNK) {
          const detailRes = await getMusicDetail(songIds.slice(offset, offset + CHUNK));
          if (!detailRes?.songs) continue;
          for (const song of detailRes.songs) {
            songs.push(
              asRawEntry({
                id: song.id,
                name: song.name,
                artist: song.ar,
                album: song.al,
                alia: song.alia,
                time: formatSongTime(song.dt),
                fee: song.fee,
                pc: song.pc || null,
                mv: song.mv || null,
              }) as SongData,
            );
          }
        }
        if (!songs.length) return false;

        // 记下这份签名再采纳：上报路径拿它比对，就不会把刚收到的列表推回去。
        this.lastPlaylistSignature = songs.map((song) => song.id).join(",");
        useMusicDataStore().adoptSharedPlaylist(songs);
        return true;
      } catch (error) {
        console.error("Fetch room playlist failed:", error);
        return false;
      }
    },

    /**
     * 轮询房间状态
     */
    async pollRoomStatus(): Promise<void> {
      if (!this.roomId) return;

      try {
        const res = await listenTogether.getRoomStatus();
        if (res.code === 200 && res.data) {
          if (!res.data.inRoom) {
            $message.warning(getLanguageData("ltRoomClosed"));
            this.resetState();
            return;
          }
          // 房间确认还在 —— 刷新恢复窗口的基准。后端接管心跳后，这是 JS 侧
          // 唯一还能观测到「房间活着」的地方。
          this.markRoomAlive();

          // 低频拉一次共享列表。`RoomStatus` 不带列表版本号，所以没有别的办法
          // 观测到「对方加了歌但还没播」；`fetchRoomPlaylist` 会先比签名，
          // 列表没变时只花一个便宜的 id 请求。
          this.playlistPollTicks++;
          if (this.playlistPollTicks >= PLAYLIST_POLL_EVERY_N_STATUS) {
            this.playlistPollTicks = 0;
            void this.fetchRoomPlaylist();
          }
          // 排除房主自身，避免 onlineCount 重复计数（getter 会 +1）
          const allUsers = res.data.roomInfo?.roomUsers || [];
          this.users = this.roomInfo
            ? allUsers.filter((u) => u.userId !== this.roomInfo!.creatorId)
            : allUsers;

          // 房主和房客都处理远程同步（双向切歌同步）
          if (this.isInRoom && res.data.currentSongId) {
            await this.handleRemoteSync(res.data);
          }
        }
      } catch (error) {
        console.error("Poll room status failed:", error);
        this.syncStatus = "error";
      }
    },

    /**
     * 处理远程同步（房主和房客均可）
     * 当服务端报告的 currentSongId 与本地不同时，切换到对应歌曲
     */
    async handleRemoteSync(data: RoomStatus): Promise<void> {
      if (this.isProcessingRemoteCommand) return;

      const musicStore = useMusicDataStore();
      this.isProcessingRemoteCommand = true;

      try {
        if (data.currentSongId && data.currentSongId !== this.lastSyncedSongId) {
          const currentSong = musicStore.getPlaySongData;
          if (!currentSong || currentSong.id !== data.currentSongId) {
            let playlist = musicStore.getPlaylists;
            let index = playlist.findIndex((s) => s.id === data.currentSongId);

            // 歌曲不在本地播放列表中，重新获取房间播放列表
            if (index === -1) {
              await this.fetchRoomPlaylist();
              playlist = musicStore.getPlaylists;
              index = playlist.findIndex((s) => s.id === data.currentSongId);
            }

            // 仍然找不到，单独获取歌曲详情并添加到播放列表
            if (index === -1) {
              try {
                const detailRes = await getMusicDetail([data.currentSongId]);
                if (detailRes.songs?.length) {
                  const song = detailRes.songs[0];
                  const newSong = asRawEntry({
                    id: song.id,
                    name: song.name,
                    artist: song.ar,
                    album: song.al,
                    alia: song.alia,
                    time: formatSongTime(song.dt),
                    fee: song.fee,
                    pc: song.pc || null,
                    mv: song.mv || null,
                  });
                  musicStore.persistData.playlists.push(newSong);
                  index = musicStore.persistData.playlists.length - 1;
                }
              } catch (e) {
                console.error("Fetch song detail for sync failed:", e);
              }
            }

            if (index !== -1) {
              musicStore.commitPlaySongIndex(index);
            }
            // 无论是否找到，都更新 lastSyncedSongId 防止重复请求
            this.lastSyncedSongId = data.currentSongId;
          } else {
            // 已经在播放正确的歌曲
            this.lastSyncedSongId = data.currentSongId;
          }
        }

        if (data.playStatus && data.currentProgress !== undefined) {
          const progressSec = data.currentProgress / 1000;
          const currentTime = musicStore.getPlaySongPlaybackCurrentTime();

          if (Math.abs(currentTime - progressSec) > 3) {
            if (typeof $player !== "undefined" && $player.seek) {
              $player.seek(progressSec);
            }
          }

          const currentPlayState = musicStore.getPlayState;
          if (data.playStatus === "PLAY" && !currentPlayState) {
            musicStore.setPlayState(true);
          } else if (data.playStatus === "PAUSE" && currentPlayState) {
            musicStore.setPlayState(false);
          }

          this.lastSyncedProgress = data.currentProgress;
          this.lastSyncedStatus = data.playStatus;
        }
      } finally {
        this.isProcessingRemoteCommand = false;
      }
    },

    startPolling(): void {
      this.stopPolling();
      this.pollTimer = window.setInterval(() => {
        this.pollRoomStatus();
      }, 3000);
    },

    stopPolling(): void {
      if (this.pollTimer) {
        clearInterval(this.pollTimer);
        this.pollTimer = null;
      }
    },

    /**
     * 发送一次心跳（携带完整播放信息）
     * 服务端依赖心跳中的 songId/playStatus/progress 维持房间 CONNECTED 状态
     */
    sendHeartbeat(): void {
      if (!this.roomId) return;
      const musicStore = useMusicDataStore();
      const currentSong = musicStore.getPlaySongData;
      if (!currentSong?.id) return;

      const progress = Math.floor(musicStore.getPlaySongPlaybackCurrentTime() * 1000);
      const playStatus: PlayStatus = musicStore.getPlayState ? "PLAY" : "PAUSE";

      listenTogether
        .heartbeat({
          roomId: this.roomId,
          songId: currentSong.id,
          playStatus,
          progress,
        })
        .then((res) => {
          // 心跳成功是「房间此刻还活着」的最新证据，恢复窗口以此为基准。
          if (res?.code === 200) this.lastAliveAt = Date.now();
        })
        .catch((err) => {
          console.error("Heartbeat failed:", err);
        });
    },

    startHeartbeat(): void {
      if (!this.isInRoom) return;
      this.stopHeartbeat();
      // 三个调用点（建房 / 入房 / 恢复）都是刚从服务端拿到 200 才走到这里，
      // 所以此刻就是一次确认。先记下再发，避免首个心跳在途时 WebView 就没了，
      // 留下一个 lastAliveAt=0 的房间恢复不了。
      this.lastAliveAt = Date.now();

      // Tauri：把保活交给后端，它能活过 WebView 销毁。JS 侧一个定时器都不留，
      // 避免两边同时上报造成状态抖动。
      if (setNativeListenTogetherRoom(this.roomId)) return;

      // Web（或原生控制器尚未创建）：只能自己跑。
      this.sendHeartbeat();
      this.heartbeatTimer = window.setInterval(() => {
        this.sendHeartbeat();
      }, 30000);
    },

    stopHeartbeat(): void {
      setNativeListenTogetherRoom(null);
      if (this.heartbeatTimer) {
        clearInterval(this.heartbeatTimer);
        this.heartbeatTimer = null;
      }
    },

    /**
     * 刷新「房间还活着」的时间戳（恢复窗口的基准）。
     *
     * 后端接管心跳后 JS 不再有心跳回调，所以改由轮询成功来喂。轮询是 3s 一次，
     * 而这个值要落持久化，所以按 30s 节流 —— 与心跳周期同量级即可。
     */
    markRoomAlive(): void {
      const now = Date.now();
      if (now - this.lastAliveAt < 30_000) return;
      this.lastAliveAt = now;
    },

    resetState(): void {
      this.enabled = false;
      this.roomInfo = null;
      this.role = "none";
      this.users = [];
      this.clientSeq = 0;
      this.syncStatus = "idle";
      this.stopPolling();
      this.stopHeartbeat();
      this.lastSyncedSongId = null;
      this.lastSyncedProgress = 0;
      this.lastSyncedStatus = "STOP";
      this.isProcessingRemoteCommand = false;
      this.lastAliveAt = 0;
      this.lastPlaylistSignature = "";
      this.playlistPollTicks = 0;
      // 房间的存在与否决定后端能不能自己决定下一首。不重新发布的话，门闸会一直
      // 关到下一次曲目开始（handleNativePlay 才会发布），这中间后端不会接管
      // 换歌 —— 恰好是启动时探活失败那一下最需要它的时候。
      publishNativeManifest();
    },

    async joinFromUrl(): Promise<boolean> {
      const urlParams = new URLSearchParams(window.location.search);
      const roomId = urlParams.get("listenTogether");
      if (roomId) {
        const newUrl = window.location.pathname + window.location.hash;
        window.history.replaceState({}, "", newUrl);
        return await this.joinRoom(roomId);
      }
      return false;
    },

    /**
     * 恢复重载前的房间。
     *
     * 房间状态此前完全不持久化，而心跳 / 轮询都挂在 `setInterval` 上：Android 的
     * WebView 一被销毁重载，roomId 就没了 —— 心跳停掉后服务端把你标记为断开，
     * 远端的切歌 / 暂停也再收不到，而 `joinFromUrl()` 救不回来（进房时
     * `history.replaceState` 已经把 `?listenTogether=` 抹掉了）。
     *
     * 这里只做「回到房间」，补不了重载那几秒的空窗；真正的修法是把心跳和轮询
     * 移出 WebView（见 docs/backend-authority-subscriber-ipc-plan.md 的 Phase 3）。
     *
     * 探活用 `getRoomStatus()`：房间可能在我们离线期间已经被房主解散。
     */
    async resumePersistedRoom(): Promise<boolean> {
      if (!this.roomId) return false;

      // 恢复窗口：心跳每 30s 刷新一次 lastAliveAt，所以死亡瞬间最多滞后 30s；
      // WebView 重建只隔几秒，而隔天再开必然远超上界。
      const age = Date.now() - this.lastAliveAt;
      if (!(this.lastAliveAt > 0) || age > ROOM_RESUME_MAX_AGE_MS) {
        this.resetState();
        return false;
      }

      const userStore = useUserDataStore();
      if (!userStore.userLogin) {
        // 登录态没了，房间必然也不成立。
        this.resetState();
        return false;
      }

      try {
        const res = await listenTogether.getRoomStatus();
        if (res.code !== 200 || !res.data?.inRoom) {
          this.resetState();
          return false;
        }

        const allUsers = res.data.roomInfo?.roomUsers || [];
        this.users = this.roomInfo
          ? allUsers.filter((u) => u.userId !== this.roomInfo!.creatorId)
          : allUsers;
        this.syncStatus = "idle";
        this.isProcessingRemoteCommand = false;

        // 重启保活与观测。startHeartbeat 会立即发一次带播放信息的心跳，
        // 让服务端尽快把房间从 NOT_CONNECTED 切回 CONNECTED。
        this.startHeartbeat();
        this.startPolling();
        return true;
      } catch (error) {
        console.error("Resume listen-together room failed:", error);
        // 探活失败可能只是网络抖动，保留房间状态等下一次轮询，不主动退房。
        return false;
      }
    },
  },

  // 只持久化「房间身份」+ 存活时间戳。计时器 id 重载后必然失效，成员列表和
  // 同步游标由恢复时的 getRoomStatus() 重新拉取。
  persist: [
    {
      storage: localStorage,
      pick: ["enabled", "roomInfo", "role", "clientSeq", "lastAliveAt"],
    },
  ],
});

/**
 * 格式化歌曲时长
 */
function formatSongTime(ms: number): string {
  const minutes = Math.floor(ms / 60000);
  const seconds = Math.floor((ms % 60000) / 1000);
  return `${minutes.toString().padStart(2, "0")}:${seconds.toString().padStart(2, "0")}`;
}

if (import.meta.hot) {
  import.meta.hot.accept(acceptHMRUpdate(useListenTogetherStore, import.meta.hot));
}

export default useListenTogetherStore;
