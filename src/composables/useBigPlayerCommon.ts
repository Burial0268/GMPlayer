import { ref, computed, nextTick, type Ref } from "vue";
import { useLayerNavigation } from "@/utils/navigation";
import { musicStore, settingStore } from "@/store";
import { setSeek } from "@/utils/AudioContext";
import { coverUrl } from "@/utils/coverUrl";

export function useBigPlayerCommon(isMobile: Ref<boolean>) {
  const navigation = useLayerNavigation();
  const music = musicStore();
  const setting = settingStore();

  // --- Cover image URLs ---
  // Through `coverUrl` because a local track's cover is an asset-protocol URL:
  // the old unconditional `http:` → `https:` rewrite turned
  // `http://asset.localhost/…` into an origin with no protocol handler, and
  // AMLL's `setAlbum` then retried a network fetch until it gave up.
  const coverImageUrl = computed(() => coverUrl(music.getPlaySongData?.album?.picUrl));

  // The resize hint is Netease's; a file on disk is served as-is.
  const coverImageUrl500 = computed(() => coverUrl(music.getPlaySongData?.album?.picUrl, 500));

  // --- Song metadata ---
  const artistList = computed(() => music.getPlaySongData?.artist ?? []);
  const songName = computed(() => music.getPlaySongData?.name ?? "");

  // --- Remaining time ---
  const remainingTime = computed(() => {
    const playTime = music.getPlaySongTime;
    if (!playTime || !playTime.duration) return "0:00";
    const remaining = Math.max(0, playTime.duration - (playTime.currentTime || 0));
    const minutes = Math.floor(remaining / 60);
    const seconds = Math.floor(remaining % 60);
    return `${minutes}:${seconds.toString().padStart(2, "0")}`;
  });

  // --- Has lyrics ---
  const hasLyrics = computed(() => {
    const lrc = music.getPlaySongLyric?.lrc;
    return !!(lrc && lrc[0] && lrc.length > 4);
  });

  // --- Lyric mouse/scroll ---
  const lrcMouseStatus = ref(false);

  const lyricsScroll = (index: number) => {
    const lrcType = !music.getPlaySongLyric.hasYrc || !setting.showYrc ? "lrc" : "yrc";
    const el = document.getElementById(lrcType + index);
    if (!el || lrcMouseStatus.value) return;

    const container = el.parentElement;
    if (!container) return;

    const containerHeight = container.clientHeight;
    const scrollDistance = el.offsetTop - container.offsetTop - containerHeight * 0.35;

    container.scrollTo({
      top: scrollDistance,
      behavior: "smooth",
    });
  };

  const lrcAllLeave = () => {
    lrcMouseStatus.value = false;
    lyricsScroll(music.getPlaySongLyricIndex);
  };

  const lrcTextClick = (time: number) => {
    if (typeof window.$player !== "undefined") {
      setSeek(window.$player, time);
      music.setPlayState(true);
    }
    lrcMouseStatus.value = false;
  };

  // --- Actions ---
  const closeBigPlayer = () => {
    navigation.closeTop("player");
  };

  const handleProgressSeek = (val: number) => {
    if (typeof window.$player !== "undefined" && music.getPlaySongTime?.duration) {
      setSeek(window.$player, val);
    }
  };

  const toComment = () => {
    // A local file has no comment thread, and `/comment?id=-N` would ask Netease
    // for one anyway — the view fetches on mount. The button is hidden for local
    // tracks; this is the guard for the paths that reach the handler directly.
    if (music.getPlaySongData?.local?.uri) return;
    navigation.openPage({
      path: "/comment",
      query: {
        id: music.getPlaySongData ? music.getPlaySongData.id : null,
      },
    });
  };

  // --- Name overflow detection (mobile) ---
  const isNameOverflow = ref(false);
  const nameWrapperRef = ref<HTMLElement | null>(null);
  const nameTextRef = ref<HTMLElement | null>(null);

  const checkNameOverflow = () => {
    if (!isMobile.value) return;
    nextTick(() => {
      const wrapper = nameWrapperRef.value;
      const text = nameTextRef.value;
      if (wrapper && text) {
        const inner = text.querySelector(".name-inner") as HTMLElement | null;
        if (inner) {
          isNameOverflow.value = inner.scrollWidth > wrapper.clientWidth;
        }
      }
    });
  };

  return {
    coverImageUrl,
    coverImageUrl500,
    artistList,
    songName,
    remainingTime,
    hasLyrics,
    lrcMouseStatus,
    lyricsScroll,
    lrcAllLeave,
    lrcTextClick,
    closeBigPlayer,
    handleProgressSeek,
    toComment,
    isNameOverflow,
    nameWrapperRef,
    nameTextRef,
    checkNameOverflow,
  };
}
