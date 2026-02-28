/**
 * Listen Together API - 一起听歌接口
 * 基于网易云音乐一起听歌功能
 */

import request from "@/utils/request";

/**
 * 播放命令类型
 */
export type PlayCommandType = "PLAY" | "PAUSE" | "GOTO" | "seek" | "NONE";

/**
 * 播放状态
 */
export type PlayStatus = "PLAY" | "PAUSE" | "STOP";

/**
 * 歌单同步命令类型
 */
export type PlaylistCommandType = "ADD" | "DEL" | "REPLACE" | "MOVE";

/**
 * 房间中的用户信息
 * 以实际接口返回为准：nickname 全小写，新增 identity 相关字段
 */
export interface RoomUser {
  userId: number;
  nickname: string;             // 实际返回字段为 nickname，非 nickName
  avatarUrl: string;
  outerId: string | null;
  identityIcon: string | null;
  identityName: string | null;
  identityTag: string | null;
}

/**
 * 房间信息
 * 以 createRoom 实际响应为准：
 *   - 无 hostNickName / hostAvatarUrl / hostBackgroundUrl
 *   - 房主通过 creatorId 在 roomUsers 中匹配
 *   - roomUsers 直接内嵌在 roomInfo 中
 */
export interface RoomInfo {
  roomId: string;
  creatorId: number;
  effectiveDurationMs: number;
  waitMs: number;
  roomCreateTime: number;
  chatRoomId: string;
  agoraChannelId: string;
  roomUsers: RoomUser[];
  roomRTCType: string | null;
  roomType: string;
  ltType: number;
  openHeartRcmd: boolean;
}

/**
 * 播放命令参数
 */
export interface PlayCommandParams {
  roomId: string;
  progress: number;
  commandType: PlayCommandType;
  formerSongId: string;
  targetSongId: number;
  clientSeq: number;
  playStatus: PlayStatus;
}

/**
 * 歌单同步参数
 * displayList / randomList 为逗号拼接的字符串
 */
export interface SyncPlaylistParams {
  roomId: string;
  commandType: PlaylistCommandType;
  userId: number;
  version: number;
  playMode?: string;
  displayList?: string;
  randomList?: string;
}

/**
 * 心跳参数
 * 心跳需要携带当前播放状态，服务端用于维持房间连接状态
 */
export interface HeartbeatParams {
  roomId: string;
  songId: number;
  playStatus: PlayStatus;
  progress: number;
}

/**
 * 房间状态响应
 */
export interface RoomStatus {
  inRoom: boolean;
  roomInfo: {
    roomUsers: RoomUser[];
  };
  currentSongId?: number;
  currentProgress?: number;
  playStatus?: PlayStatus;
}

/**
 * 一起听歌 API
 */
export const listenTogether = {
  /**
   * 创建听歌房间
   * 实际响应结构：{ code, data: { type, roomInfo: RoomInfo, inviteUserInfo, hintText, toastText } }
   */
  createRoom: () =>
    request<{
      code: number;
      message?: string;
      data?: {
        type: string;
        roomInfo: RoomInfo;
        inviteUserInfo: null;
        hintText: string | null;
        toastText: string | null;
      };
    }>({
      method: "GET",
      url: "/listentogether/room/create",
      params: { timestamp: Date.now() },
    }),

  /**
   * 检查房间是否存在
   */
  checkRoom: (roomId: string) =>
    request<{
      code: number;
      message?: string;
      data?: {
        roomId: string;
        hostUserId: number;
      };
    }>({
      method: "POST",
      url: "/listentogether/room/check",
      data: { roomId },
    }),

  /**
   * 加入听歌房间
   */
  joinRoom: (roomId: string, inviterId?: number) =>
    request<{
      code: number;
      message?: string;
      data?: {
        roomId: string;
        hostUserId: number;
      };
    }>({
      method: "POST",
      url: "/listentogether/accept",
      data: { roomId, inviterId },
    }),

  /**
   * 获取房间播放列表
   */
  getPlaylist: (roomId: string) =>
    request<{
      code: number;
      message?: string;
      data?: {
        playlist: {
          displayList: { result: number[] };
          randomList: { result: number[] };
          playMode: string;
        };
      };
    }>({
      method: "POST",
      url: "/listentogether/sync/playlist/get",
      data: { roomId },
    }),

  /**
   * 发送播放命令
   */
  sendPlayCommand: (params: PlayCommandParams) =>
    request<{
      code: number;
      message?: string;
      data?: {
        clientSeq: number;
      };
    }>({
      method: "POST",
      url: "/listentogether/play/command",
      data: params,
    }),

  /**
   * 同步播放列表
   */
  syncPlaylist: (params: SyncPlaylistParams) =>
    request<{
      code: number;
      message?: string;
    }>({
      method: "POST",
      url: "/listentogether/sync/list/command",
      data: params,
    }),

  /**
   * 获取房间状态（轮询）
   */
  getRoomStatus: () =>
    request<{
      code: number;
      message?: string;
      data?: RoomStatus;
    }>({
      method: "GET",
      url: "/listentogether/status",
      params: { timestamp: Date.now() },
    }),

  /**
   * 结束房间（房主）
   */
  endRoom: (roomId: string) =>
    request<{
      code: number;
      message?: string;
      data?: { success: boolean };
    }>({
      method: "POST",
      url: "/listentogether/end",
      data: { roomId },
    }),

  /**
   * 退出房间（普通用户）
   */
  leaveRoom: (roomId: string) =>
    request<{
      code: number;
      message?: string;
    }>({
      method: "POST",
      url: "/listentogether/leave",
      data: { roomId },
    }),

  /**
   * 心跳保活
   * 需携带当前播放信息，服务端据此维持房间 CONNECTED 状态
   * 注意：NeteaseCloudMusicApi 代理路由为 heatbeat（缺少 r，是 API 已知 typo）
   */
  heartbeat: (params: HeartbeatParams) =>
    request<{
      code: number;
      message?: string;
    }>({
      method: "POST",
      url: "/listentogether/heatbeat",
      data: params,
      hiddenBar: true,
    }),
};

export default listenTogether;

// Legacy exports
export const createListenRoom = listenTogether.createRoom;
export const checkListenRoom = listenTogether.checkRoom;
export const joinListenRoom = listenTogether.joinRoom;
export const getListenRoomPlaylist = listenTogether.getPlaylist;
export const sendListenCommand = listenTogether.sendPlayCommand;
export const syncListenPlaylist = listenTogether.syncPlaylist;
export const getListenRoomStatus = listenTogether.getRoomStatus;
export const endListenRoom = listenTogether.endRoom;
export const leaveListenRoom = listenTogether.leaveRoom;