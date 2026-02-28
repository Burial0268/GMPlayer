/**
 * API Module - Netease Cloud Music API
 * 网易云音乐 API 模块
 */

// Types
export * from "./types";

// API modules
export { album } from "./album";
export { artist } from "./artist";
export { comment } from "./comment";
export { home } from "./home";
export { listenTogether } from "./listenTogether";
export { login } from "./login";
export { playlist } from "./playlist";
export { search } from "./search";
export { song } from "./song";
export { user } from "./user";
export { video } from "./video";

// Import for default export
import { album } from "./album";
import { artist } from "./artist";
import { comment } from "./comment";
import { home } from "./home";
import { listenTogether } from "./listenTogether";
import { login } from "./login";
import { playlist } from "./playlist";
import { search } from "./search";
import { song } from "./song";
import { user } from "./user";
import { video } from "./video";

// Default export
const api = {
  album,
  artist,
  comment,
  home,
  listenTogether,
  login,
  playlist,
  search,
  song,
  user,
  video,
};

export default api;

// ============== Legacy Exports (for backward compatibility) ==============

// Album
export const getAlbum = album.get;
export const getAlbumNew = album.getNew;
export const getNewAlbum = album.getNewest;
export const getToplist = home.getToplist;
export const likeAlbum = album.subscribe;

// Artist
export const getArtistList = artist.getList;
export const getArtistDetail = artist.getDetail;
export const getArtistSongs = artist.getSongs;
export const getArtistAllSongs = artist.getAllSongs;
export const getArtistAblums = artist.getAlbums;
export const getArtistVideos = artist.getVideos;
export const likeArtist = artist.subscribe;
export const getTopArtists = artist.getTop;

// Comment
export const getComment = comment.get;
export const likeComment = comment.like;
export const sendComment = comment.send;

// Home
export const getBanner = home.getBanner;
export const getDailySongs = home.getDailySongs;
export const getDailySongsHistory = home.getDailySongsHistory;
export const getDailySongsHistoryDetail = home.getDailySongsHistoryDetail;
export const getPersonalFm = home.getPersonalFm;
export const setFmTrash = home.setFmTrash;
export const getPersonalized = home.getPersonalized;

// Login
export const getQrKey = login.getQrKey;
export const qrCreate = login.createQr;
export const checkQr = login.checkQr;
export const toLogin = login.byPhone;
export const sentCaptcha = login.sendCaptcha;
export const verifyCaptcha = login.verifyCaptcha;
export const getLoginState = login.getStatus;
export const refreshLogin = login.refresh;
export const userLogOut = login.logout;

// Playlist
export const getPlayListCatlist = playlist.getCatlist;
export const getTopPlaylist = playlist.getTop;
export const getHighqualityPlaylist = playlist.getHighquality;
export const getPlayListDetail = playlist.getDetail;
export const getAllPlayList = playlist.getTracks;
export const createPlaylist = playlist.create;
export const delPlayList = playlist.delete;
export const playlistUpdate = playlist.update;
export const addSongToPlayList = playlist.updateTracks;
export const likePlaylist = playlist.subscribe;

// Search
export const getSearchHot = search.getHot;
export const getSearchSuggest = search.getSuggest;
export const getSearchData = search.search;

// Song
export const checkMusicCanUse = song.checkAvailable;
export const getMusicUrl = song.getUrl;
export const getMusicNumUrl = song.getUnmUrl;
export const getUnifiedLyric = song.getLyric;
export const checkLyricMeta = song.checkLyricMeta;
export const getMusicDetail = song.getDetail;
export const getSimiPlayList = song.getSimiPlaylist;
export const getSimiSong = song.getSimiSong;
export const getSongDownload = song.getDownloadUrl;
export const songScrobble = song.scrobble;
export const setLikeSong = user.likeSong;

// User
export const getUserLevel = user.getLevel;
export const getUserSubcount = user.getSubcount;
export const getUserPlaylist = user.getPlaylist;
export const getUserAlbum = album.getSublist;
export const getUserArtistlist = artist.getSublist;
export const getLikelist = user.getLikelist;
export const getCloud = user.getCloud;
export const setCloudDel = user.deleteCloudSong;
export const setCloudMatch = user.matchCloudSong;
export const upCloudSong = user.uploadCloudSong;
export const userDailySignin = user.dailySignin;
export const userYunbeiSign = user.yunbeiSign;

// Listen Together
export {
  createListenRoom,
  checkListenRoom,
  joinListenRoom,
  getListenRoomPlaylist,
  sendListenCommand,
  syncListenPlaylist,
  getListenRoomStatus,
  endListenRoom,
  leaveListenRoom,
} from "./listenTogether";

// Video
export const getVideoDetail = video.getDetail;
export const getVideoUrl = video.getUrl;
export const getSimiVideo = video.getSimi;
