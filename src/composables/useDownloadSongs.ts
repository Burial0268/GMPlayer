/**
 * "Download these songs" as one action.
 *
 * Exists so every batch entry point is two lines and they cannot drift: the
 * mapping from a `SongData` row to a queue request, the free/VIP pre-check, and
 * the confirmation are all decided once here.
 */
import { useI18n } from "vue-i18n";
import { useDownloadStore, userStore } from "@/store";
import { downloadAvailable, type DownloadRequest } from "@/utils/download";
import { isLocalSong } from "@/utils/localLibrary";
import type { SongData } from "@/store/musicTypes";

/**
 * Above this many songs, ask first.
 *
 * Not a limit — the queue is happy with more. It is that "download all" on a
 * 900-track playlist is a decision, and a mis-click that starts several gigabytes
 * of transfers is not something to discover from a progress bar.
 */
const CONFIRM_ABOVE = 20;

const toRequest = (song: SongData): DownloadRequest => ({
  songId: song.id,
  title: song.name ?? "",
  artist: song.artist?.[0]?.name ?? "",
  album: song.album?.name ?? "",
  coverUrl: song.album?.picUrl ?? undefined,
  // `br` is left off on purpose: the queue applies the configured default, so a
  // batch follows the setting rather than freezing whatever it was when the
  // button was pressed.
});

export const useDownloadSongs = () => {
  const { t } = useI18n();
  const user = userStore();
  const download = useDownloadStore();

  /**
   * Which of `songs` can actually be downloaded.
   *
   * A local file has no Netease id to resolve (its `SongData.id` is a negative
   * hash that only exists in this process), and a paid track needs either VIP or
   * the cloud-disk copy — the same test `DownloadSong.vue` applies to one song.
   */
  const downloadable = (songs: SongData[]): SongData[] => {
    const vip = Boolean(user.userData?.vipType);
    return songs.filter(
      (song) => song?.id && !isLocalSong(song) && (vip || song.fee === 0 || Boolean(song.pc)),
    );
  };

  const enqueue = async (songs: SongData[]): Promise<void> => {
    if (!downloadAvailable()) {
      $message.error(t("download.tauriOnly"));
      return;
    }
    if (!user.userLogin) {
      $message.error(t("general.message.needLogin"));
      return;
    }
    const eligible = downloadable(songs ?? []);
    if (!eligible.length) {
      $message.error(t("download.nothingDownloadable"));
      return;
    }

    const run = async () => {
      try {
        const added = await download.enqueue(eligible.map((song) => toRequest(song)));
        if (added > 0) {
          $message.success(t("download.queued", { count: added }));
        } else {
          // Either every one of them is already queued, or the user cancelled the
          // directory picker. Both are "nothing happened", not a failure.
          $message.info(t("download.queuedNone"));
        }
      } catch (error) {
        console.error("[download] enqueue failed:", error);
        $message.error(t("general.message.downloadError"));
      }
    };

    if (eligible.length <= CONFIRM_ABOVE) {
      await run();
      return;
    }
    $dialog.warning({
      class: "s-dialog",
      title: t("download.downloadAll"),
      content: t("download.confirmMany", {
        count: eligible.length,
        skipped: songs.length - eligible.length,
      }),
      positiveText: t("general.dialog.confirm"),
      negativeText: t("general.dialog.cancel"),
      onPositiveClick: run,
    });
  };

  return { enqueue, downloadable };
};
