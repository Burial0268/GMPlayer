import type { RouteRecordRaw } from "vue-router";

const routes: RouteRecordRaw[] = [
  {
    path: "/",
    name: "home",
    meta: {
      title: "首页",
    },
    component: () => import("@/views/Home/HomeView.vue"),
  },
  // 搜索页
  {
    path: "/search",
    name: "search",
    meta: {
      title: "搜索",
    },
    component: () => import("@/views/Search/index.vue"),
    redirect: "/search/songs",
    children: [
      {
        path: "songs",
        name: "s-songs",
        component: () => import("@/views/Search/songs.vue"),
      },
      {
        path: "artists",
        name: "s-artists",
        component: () => import("@/views/Search/artists.vue"),
      },
      {
        path: "albums",
        name: "s-albums",
        component: () => import("@/views/Search/albums.vue"),
      },
      {
        path: "videos",
        name: "s-videos",
        component: () => import("@/views/Search/videos.vue"),
      },
      {
        path: "playlists",
        name: "s-playlists",
        component: () => import("@/views/Search/playlists.vue"),
      },
      {
        path: "users",
        name: "s-users",
        component: () => import("@/views/Search/users.vue"),
      },
    ],
  },
  // 发现页
  {
    path: "/discover",
    name: "discover",
    meta: {
      title: "发现",
    },
    component: () => import("@/views/Discover/index.vue"),
    redirect: "/discover/playlists",
    children: [
      {
        path: "playlists",
        name: "dsc-playlists",
        component: () => import("@/views/Discover/playlists.vue"),
      },
      {
        path: "toplists",
        name: "dsc-toplists",
        component: () => import("@/views/Discover/toplists.vue"),
      },
      {
        path: "artists",
        name: "dsc-artists",
        component: () => import("@/views/Discover/artists.vue"),
      },
    ],
  },
  // 我的页面
  {
    path: "/user",
    name: "user",
    meta: {
      title: "我的",
      needLogin: true,
    },
    component: () => import("@/views/User/index.vue"),
    redirect: "/user/playlists",
    children: [
      {
        path: "playlists",
        name: "user-playlists",
        component: () => import("@/views/User/playlists.vue"),
      },
      {
        path: "like",
        name: "user-like",
        component: () => import("@/views/User/like.vue"),
      },
      {
        path: "album",
        name: "user-album",
        component: () => import("@/views/User/album.vue"),
      },
      {
        path: "artists",
        name: "user-artists",
        component: () => import("@/views/User/artists.vue"),
      },
      {
        path: "cloud",
        name: "user-cloud",
        component: () => import("@/views/User/cloud.vue"),
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
      title: "本地音乐",
    },
    component: () => import("@/views/Local/index.vue"),
    redirect: "/local/songs",
    children: [
      {
        path: "songs",
        name: "local-songs",
        component: () => import("@/views/Local/songs.vue"),
      },
      {
        path: "albums",
        name: "local-albums",
        component: () => import("@/views/Local/albums.vue"),
      },
      {
        path: "artists",
        name: "local-artists",
        component: () => import("@/views/Local/artists.vue"),
      },
      {
        path: "folders",
        name: "local-folders",
        component: () => import("@/views/Local/folders.vue"),
      },
      {
        path: "playlists",
        name: "local-playlists",
        component: () => import("@/views/Local/playlists.vue"),
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
      title: "本地音乐",
    },
    component: () => import("@/views/Local/LocalPlaylistView.vue"),
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
      title: "本地音乐",
    },
    component: () => import("@/views/Local/LocalSongView.vue"),
  },
  // 用户主页（公开，可查看任意用户）
  {
    path: "/profile",
    name: "profile",
    meta: {
      title: "用户主页",
    },
    component: () => import("@/views/Profile/index.vue"),
  },
  // 评论页
  {
    path: "/comment",
    name: "comment",
    meta: {
      title: "歌曲评论",
    },
    component: () => import("@/views/Comment/CommentView.vue"),
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
      title: "全局设置",
      hideLoadingBar: true,
    },
    component: () => import("@/views/Setting/index.vue"),
  },
  // 登录页
  {
    path: "/login",
    name: "login",
    meta: {
      title: "登录",
    },
    component: () => import("@/views/Login/LoginView.vue"),
  },
  // 视频页
  {
    path: "/video",
    name: "video",
    meta: {
      title: "视频",
    },
    component: () => import("@/views/Video/VideoView.vue"),
  },
  // 歌单页
  {
    path: "/playlist",
    name: "playlist",
    meta: {
      title: "歌单",
    },
    component: () => import("@/views/PlayList/PlayListView.vue"),
  },
  // 歌曲页
  {
    path: "/song",
    name: "song",
    meta: {
      title: "歌曲",
    },
    component: () => import("@/views/Song/SongView.vue"),
  },
  // 每日推荐
  {
    path: "/dailySongs",
    name: "dailySongs",
    meta: {
      title: "每日推荐",
      needLogin: true,
    },
    component: () => import("@/views/DailySongs/DailySongsView.vue"),
  },
  // 专辑页
  {
    path: "/album",
    name: "album",
    meta: {
      title: "专辑",
    },
    component: () => import("@/views/Album/AlbumView.vue"),
  },
  // 歌手页
  {
    path: "/artist",
    name: "artist",
    meta: {
      title: "歌手",
    },
    component: () => import("@/views/Artist/index.vue"),
    redirect: "/artist/songs",
    children: [
      {
        path: "songs",
        name: "ar-songs",
        component: () => import("@/views/Artist/songs.vue"),
      },
      {
        path: "albums",
        name: "ar-albums",
        component: () => import("@/views/Artist/albums.vue"),
      },
      {
        path: "videos",
        name: "ar-videos",
        component: () => import("@/views/Artist/videos.vue"),
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
    component: () => import("@/views/Artist/all-songs.vue"),
  },
  // 历史记录
  {
    path: "/history",
    name: "history",
    meta: {
      title: "history",
    },
    component: () => import("@/views/History/HistoryView.vue"),
  },
  // 全部新碟
  {
    path: "/new-album",
    name: "new-album",
    meta: {
      title: "全部新碟",
    },
    component: () => import("@/views/NewAlbum/NewAlbumView.vue"),
  },
  // 状态页
  // 404
  {
    path: "/404",
    name: "404",
    meta: {
      title: "404",
    },
    component: () => import("@/views/State/404.vue"),
  },
  // 403
  {
    path: "/403",
    name: "403",
    meta: {
      title: "403",
    },
    component: () => import("@/views/State/403.vue"),
  },
  // 500
  {
    path: "/500",
    name: "500",
    meta: {
      title: "500",
    },
    component: () => import("@/views/State/500.vue"),
  },
  {
    path: "/:pathMatch(.*)",
    redirect: "/404",
  },
];

export default routes;
