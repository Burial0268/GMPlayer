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
      const musicData = useMusicDataStore();
      if (!state.roomInfo) return "";
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

          $message.success("一起听房间创建成功");
          return true;
        } else {
          $message.error(res.message || "listenTogether.createFailed");
          return false;
        }
      } catch (error) {
        console.error("Create room failed:", error);
        $message.error("listenTogether.createFailed");
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
          this.startPolling();

          $message.success("listenTogether.joinSuccess");
          return true;
        } else {
          $message.error(res.message || "listenTogether.joinFailed");
          return false;
        }
      } catch (error) {
        console.error("Join room failed:", error);
        $message.error("listenTogether.joinFailed");
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
          this.role === "host" ? "listenTogether.closeSuccess" : "listenTogether.leaveSuccess",
        );
      } catch (error) {
        console.error("Leave room error:", error);
      } finally {
        this.resetState();
      }
    },

    /**
     * 发送播放命令（房主）
     */
    async sendPlayCommand(
      commandType: "PLAY" | "PAUSE" | "GOTO" | "seek",
      progress?: number,
    ): Promise<boolean> {
      if (!this.isHost || !this.roomInfo) return false;

      const musicStore = useMusicDataStore();
      const currentSong = musicStore.getPlaySongData;
      if (!currentSong?.id) return false;

      try {
        const currentProgress =
          progress ?? Math.floor((musicStore.getPlaySongTime.currentTime || 0) * 1000);

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
     * 同步当前播放列表到房间（房主）
     */
    async syncCurrentPlaylist(): Promise<boolean> {
      if (!this.isHost || !this.roomInfo) return false;

      const musicStore = useMusicDataStore();
      const userStore = useUserDataStore();
      const playlist = musicStore.getPlaylists;
      if (!playlist.length) return false;

      try {
        const trackIds = playlist.map((song) => song.id).join(",");

        const res = await listenTogether.syncPlaylist({
          roomId: this.roomInfo.roomId,
          commandType: "REPLACE",
          userId: userStore.userData.userId || 0,
          version: this.clientSeq++,
          playMode: musicStore.getPlaySongMode.toUpperCase(),
          displayList: trackIds,
          randomList: trackIds,
        });

        return res.code === 200;
      } catch (error) {
        console.error("Sync playlist failed:", error);
        return false;
      }
    },

    /**
     * 获取房间播放列表（房客）
     */
    async fetchRoomPlaylist(): Promise<boolean> {
      if (!this.roomId) return false;

      try {
        const res = await listenTogether.getPlaylist(this.roomId);
        if (res.code === 200 && res.data?.playlist?.displayList?.result) {
          const musicStore = useMusicDataStore();
          const songIds = res.data.playlist.displayList.result.slice(0, 100);

          if (songIds.length > 0) {
            const detailRes = await getMusicDetail(songIds);
            if (detailRes.songs) {
              const songs = detailRes.songs.map((song: any) => ({
                id: song.id,
                name: song.name,
                artist: song.ar,
                album: song.al,
                alia: song.alia,
                time: formatSongTime(song.dt),
                fee: song.fee,
                pc: song.pc || null,
                mv: song.mv || null,
              }));
              musicStore.setPlaylists(songs);
            }
          }
          return true;
        }
        return false;
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
            $message.warning("listenTogether.roomClosed");
            this.resetState();
            return;
          }

          // 排除房主自身，避免 onlineCount 重复计数（getter 会 +1）
          const allUsers = res.data.roomInfo?.roomUsers || [];
          this.users = this.roomInfo
            ? allUsers.filter((u) => u.userId !== this.roomInfo!.creatorId)
            : allUsers;

          if (this.isGuest && res.data.currentSongId) {
            await this.handleRemoteSync(res.data);
          }
        }
      } catch (error) {
        console.error("Poll room status failed:", error);
        this.syncStatus = "error";
      }
    },

    /**
     * 处理远程同步（房客）
     */
    async handleRemoteSync(data: RoomStatus): Promise<void> {
      if (this.isProcessingRemoteCommand) return;

      const musicStore = useMusicDataStore();
      this.isProcessingRemoteCommand = true;

      try {
        if (data.currentSongId && data.currentSongId !== this.lastSyncedSongId) {
          const currentSong = musicStore.getPlaySongData;
          if (!currentSong || currentSong.id !== data.currentSongId) {
            const playlist = musicStore.getPlaylists;
            const index = playlist.findIndex((s) => s.id === data.currentSongId);
            if (index !== -1) {
              musicStore.persistData.playSongIndex = index;
              this.lastSyncedSongId = data.currentSongId;
            }
          }
        }

        if (data.playStatus && data.currentProgress !== undefined) {
          const progressSec = data.currentProgress / 1000;
          const currentTime = musicStore.getPlaySongTime.currentTime;

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

      const progress = Math.floor((musicStore.getPlaySongTime.currentTime || 0) * 1000);
      const playStatus: PlayStatus = musicStore.getPlayState ? "PLAY" : "PAUSE";

      listenTogether
        .heartbeat({
          roomId: this.roomId,
          songId: currentSong.id,
          playStatus,
          progress,
        })
        .catch((err) => {
          console.error("Heartbeat failed:", err);
        });
    },

    startHeartbeat(): void {
      if (!this.isHost) return;
      this.stopHeartbeat();
      // 立即发送一次心跳，让服务端尽快切换到 CONNECTED
      this.sendHeartbeat();
      // 之后每 30s 定时发送
      this.heartbeatTimer = window.setInterval(() => {
        this.sendHeartbeat();
      }, 30000);
    },

    stopHeartbeat(): void {
      if (this.heartbeatTimer) {
        clearInterval(this.heartbeatTimer);
        this.heartbeatTimer = null;
      }
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
  },
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
