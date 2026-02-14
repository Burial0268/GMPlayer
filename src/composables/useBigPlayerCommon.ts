import { ref, computed, nextTick, type Ref } from "vue";
import { useRouter } from "vue-router";
import { musicStore, settingStore } from "@/store";
import { setSeek } from "@/utils/AudioContext";

export function useBigPlayerCommon(isMobile: Ref<boolean>) {
  const router = useRouter();
  const music = musicStore();
  const setting = settingStore();

  // --- Cover image URLs ---
  const coverImageUrl = computed(() => {
    if (!music.getPlaySongData?.album?.picUrl) return "/images/pic/default.png";
    return music.getPlaySongData.album.picUrl.replace(/^http:/, "https:");
  });

  const coverImageUrl500 = computed(
    () => coverImageUrl.value + "?param=500y500"
  );

  // --- Song metadata ---
  const artistList = computed(() => music.getPlaySongData?.artist ?? []);
  const songName = computed(() => music.getPlaySongData?.name ?? "");

  // --- Remaining time ---
  const remainingTime = computed(() => {
    const playTime = music.getPlaySongTime;
    if (!playTime || !playTime.duration) return "0:00";
    const remaining = Math.max(
      0,
      playTime.duration - (playTime.currentTime || 0)
    );
    const minutes = Math.floor(remaining / 60);
    const seconds = Math.floor(remaining % 60);
    return `${minutes}:${seconds.toString().padStart(2, "0")}`;
  });

  // --- Has lyrics ---
  const hasLyrics = computed(() => {
    const lrc = music.getPlaySongLyric?.lrc;
    return !!(lrc && lrc[0] && lrc.length > 4);
  });

  // --- computedLowFreqVolume ---
  const computedLowFreqVolume = computed(() => {
    if (!setting.dynamicFlowSpeed) return 1.0;
    return Math.round(music.lowFreqVolume * 100) / 100;
  });

  // --- Lyric mouse/scroll ---
  const lrcMouseStatus = ref(false);

  const lyricsScroll = (index: number) => {
    const lrcType =
      !music.getPlaySongLyric.hasYrc || !setting.showYrc ? "lrc" : "yrc";
    const el = document.getElementById(lrcType + index);
    if (!el || lrcMouseStatus.value) return;

    const container = el.parentElement;
    if (!container) return;

    const containerHeight = container.clientHeight;
    const scrollDistance =
      el.offsetTop - container.offsetTop - containerHeight * 0.35;

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
      music.persistData.playSongTime.currentTime = time;
      window.$player.seek(time);
      music.setPlayState(true);
    }
    lrcMouseStatus.value = false;
  };

  // --- Actions ---
  const closeBigPlayer = () => {
    music.setBigPlayerState(false);
  };

  const handleProgressSeek = (val: number) => {
    if (
      typeof window.$player !== "undefined" &&
      music.getPlaySongTime?.duration
    ) {
      music.persistData.playSongTime.currentTime = val;
      setSeek(window.$player, val);
    }
  };

  const toComment = () => {
    music.setBigPlayerState(false);
    router.push({
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
    computedLowFreqVolume,
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
