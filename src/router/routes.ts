import type { RouteRecordRaw } from "vue-router";
import { mobileRoute } from "./mobileRoute";

const routes: RouteRecordRaw[] = [
  {
    path: "/",
    name: "home",
    meta: {
      navigationRoot: "home",
      navigationLabel: "sidebar.tab.home",
      title: "首页",
    },
    component: mobileRoute(() => import("@/views/Home/HomeView.vue")),
  },
  // 搜索页
  {
    path: "/search",
    name: "search",
    meta: {
      navigationLabel: "navigation.search",
      title: "搜索",
    },
    component: mobileRoute(() => import("@/views/Search/index.vue")),
    redirect: "/search/songs",
    children: [
      {
        path: "songs",
        name: "s-songs",
        component: mobileRoute(() => import("@/views/Search/songs.vue"), true),
      },
      {
        path: "artists",
        name: "s-artists",
        component: mobileRoute(() => import("@/views/Search/artists.vue"), true),
      },
      {
        path: "albums",
        name: "s-albums",
        component: mobileRoute(() => import("@/views/Search/albums.vue"), true),
      },
      {
        path: "videos",
        name: "s-videos",
        component: mobileRoute(() => import("@/views/Search/videos.vue"), true),
      },
      {
        path: "playlists",
        name: "s-playlists",
        component: mobileRoute(() => import("@/views/Search/playlists.vue"), true),
      },
      {
        path: "users",
        name: "s-users",
        component: mobileRoute(() => import("@/views/Search/users.vue"), true),
      },
    ],
  },
  // 发现页
  {
    path: "/discover",
    name: "discover",
    meta: {
      navigationRoot: "discover",
      navigationLabel: "sidebar.tab.discover",
      title: "发现",
    },
    component: mobileRoute(() => import("@/views/Discover/index.vue")),
    redirect: "/discover/playlists",
    children: [
      {
        path: "playlists",
        name: "dsc-playlists",
        component: mobileRoute(() => import("@/views/Discover/playlists.vue"), true),
      },
      {
        path: "toplists",
        name: "dsc-toplists",
        component: mobileRoute(() => import("@/views/Discover/toplists.vue"), true),
      },
      {
        path: "artists",
        name: "dsc-artists",
        component: mobileRoute(() => import("@/views/Discover/artists.vue"), true),
      },
    ],
  },
  // 我的页面
  {
    path: "/user",
    name: "user",
    meta: {
      navigationRoot: "library",
      navigationLabel: "sidebar.tab.library",
      title: "我的",
      needLogin: true,
    },
    component: mobileRoute(() => import("@/views/User/index.vue")),
    redirect: "/user/playlists",
    children: [
      {
        path: "playlists",
        name: "user-playlists",
        component: mobileRoute(() => import("@/views/User/playlists.vue"), true),
      },
      {
        path: "like",
        name: "user-like",
        component: mobileRoute(() => import("@/views/User/like.vue"), true),
      },
      {
        path: "album",
        name: "user-album",
        component: mobileRoute(() => import("@/views/User/album.vue"), true),
      },
      {
        path: "artists",
        name: "user-artists",
        component: mobileRoute(() => import("@/views/User/artists.vue"), true),
      },
      {
        path: "cloud",
        name: "user-cloud",
        component: mobileRoute(() => import("@/views/User/cloud.vue"), true),
      },
    ],
  },
  // 本地音乐
  //
  // 刻意**不带** `needLogin`：导入的文件夹与网易账号无关，未登录也要能用；
  // 移动端底栏在未登录时正是落到这里（见 `MobileTabBar.vue`）。
  {
    path: "/local",
    name: "local",
    meta: {
      navigationRoot: "library",
      navigationLabel: "sidebar.tab.library",
      title: "本地音乐",
    },
    component: mobileRoute(() => import("@/views/Local/index.vue")),
    redirect: "/local/songs",
    children: [
      {
        path: "songs",
        name: "local-songs",
        component: mobileRoute(() => import("@/views/Local/songs.vue"), true),
      },
      {
        path: "albums",
        name: "local-albums",
        component: mobileRoute(() => import("@/views/Local/albums.vue"), true),
      },
      {
        path: "artists",
        name: "local-artists",
        component: mobileRoute(() => import("@/views/Local/artists.vue"), true),
      },
      {
        path: "folders",
        name: "local-folders",
        component: mobileRoute(() => import("@/views/Local/folders.vue"), true),
      },
      {
        path: "playlists",
        name: "local-playlists",
        component: mobileRoute(() => import("@/views/Local/playlists.vue"), true),
      },
    ],
  },
  // 本地集合详情（全部/喜欢/专辑/艺人/文件夹/本地歌单共用一页，靠 query 区分）
  //
  // 放在 `/local` 的兄弟位置而不是子路由：它有自己的头部，不该套在标签页里。
  {
    path: "/local/playlist",
    name: "local-playlist-detail",
    meta: {
      navigationFallback: "library",
      navigationLabel: "sidebar.tab.library",
      title: "本地音乐",
    },
    component: mobileRoute(() => import("@/views/Local/LocalPlaylistView.vue")),
  },
  // 本地曲目详情：基本信息 / 元数据覆盖 / 歌词导入。
  //
  // 不复用 `/song`：那是网易云的详情页，按正数 id 拉详情、评论和相似歌单，而本地
  // 曲目的 id 是路径哈希出来的负数，在网易那边什么都不是。定位符走 query 而不是
  // 路径段——它是 Windows 路径或 `content://` URI，两者都过不了路径段。
  {
    path: "/local/song",
    name: "local-song-detail",
    meta: {
      navigationFallback: "library",
      navigationLabel: "general.name.song",
      title: "本地音乐",
    },
    component: mobileRoute(() => import("@/views/Local/LocalSongView.vue")),
  },
  // 用户主页（公开，可查看任意用户）
  {
    path: "/profile",
    name: "profile",
    meta: {
      title: "用户主页",
    },
    component: mobileRoute(() => import("@/views/Profile/index.vue")),
  },
  // 评论页
  {
    path: "/comment",
    name: "comment",
    meta: {
      navigationLabel: "general.name.comment",
      title: "歌曲评论",
    },
    component: mobileRoute(() => import("@/views/Comment/CommentView.vue")),
  },
  // 设置页
  {
    path: "/setting",
    redirect: "/setting/appearance",
  },
  {
    path: "/setting/:section",
    name: "setting",
    meta: {
      navigationRoot: "settings",
      navigationLabel: "sidebar.tab.settings",
      title: "全局设置",
      hideLoadingBar: true,
    },
    component: mobileRoute(() => import("@/views/Setting/index.vue")),
  },
  // 登录页
  {
    path: "/login",
    name: "login",
    meta: {
      title: "登录",
    },
    component: mobileRoute(() => import("@/views/Login/LoginView.vue")),
  },
  // 视频页
  {
    path: "/video",
    name: "video",
    meta: {
      title: "视频",
    },
    component: mobileRoute(() => import("@/views/Video/VideoView.vue")),
  },
  // 歌单页
  {
    path: "/playlist",
    name: "playlist",
    meta: {
      navigationDetail: "playlist",
      navigationLabel: "general.name.playlist",
      title: "歌单",
    },
    component: mobileRoute(() => import("@/views/PlayList/PlayListView.vue")),
  },
  // 歌曲页
  {
    path: "/song",
    name: "song",
    meta: {
      navigationLabel: "general.name.song",
      title: "歌曲",
    },
    component: mobileRoute(() => import("@/views/Song/SongView.vue")),
  },
  // 每日推荐
  {
    path: "/dailySongs",
    name: "dailySongs",
    meta: {
      title: "每日推荐",
      needLogin: true,
    },
    component: mobileRoute(() => import("@/views/DailySongs/DailySongsView.vue")),
  },
  // 专辑页
  {
    path: "/album",
    name: "album",
    meta: {
      navigationDetail: "album",
      navigationLabel: "general.name.album",
      title: "专辑",
    },
    component: mobileRoute(() => import("@/views/Album/AlbumView.vue")),
  },
  // 歌手页
  {
    path: "/artist",
    name: "artist",
    meta: {
      navigationDetail: "artist",
      navigationLabel: "general.name.artists",
      title: "歌手",
    },
    component: mobileRoute(() => import("@/views/Artist/index.vue")),
    redirect: "/artist/songs",
    children: [
      {
        path: "songs",
        name: "ar-songs",
        component: mobileRoute(() => import("@/views/Artist/songs.vue"), true),
      },
      {
        path: "albums",
        name: "ar-albums",
        component: mobileRoute(() => import("@/views/Artist/albums.vue"), true),
      },
      {
        path: "videos",
        name: "ar-videos",
        component: mobileRoute(() => import("@/views/Artist/videos.vue"), true),
      },
    ],
  },
  // 歌手全部歌曲
  {
    path: "/all-songs",
    name: "all-songs",
    meta: {
      title: "全部歌曲",
    },
    component: mobileRoute(() => import("@/views/Artist/all-songs.vue")),
  },
  // 历史记录
  {
    path: "/history",
    name: "history",
    meta: {
      title: "history",
    },
    component: mobileRoute(() => import("@/views/History/HistoryView.vue")),
  },
  // 全部新碟
  {
    path: "/new-album",
    name: "new-album",
    meta: {
      title: "全部新碟",
    },
    component: mobileRoute(() => import("@/views/NewAlbum/NewAlbumView.vue")),
  },
  // 状态页
  // 404
  {
    path: "/404",
    name: "404",
    meta: {
      title: "404",
    },
    component: mobileRoute(() => import("@/views/State/404.vue")),
  },
  // 403
  {
    path: "/403",
    name: "403",
    meta: {
      title: "403",
    },
    component: mobileRoute(() => import("@/views/State/403.vue")),
  },
  // 500
  {
    path: "/500",
    name: "500",
    meta: {
      title: "500",
    },
    component: mobileRoute(() => import("@/views/State/500.vue")),
  },
  {
    path: "/:pathMatch(.*)",
    redirect: "/404",
  },
];

export default routes;
