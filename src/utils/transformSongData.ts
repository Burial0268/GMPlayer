import { getSongTime } from "@/utils/timeTools";
import { ncmImageUrl } from "@/utils/ncmImageUrl";

interface TransformOptions {
  offset?: number;
  sourceId?: string | number;
  albumTransform?: (song: any) => any;
}

/** 确保 album 对象有 picUrl，缺失时从 pic_str/pic 推算 */
function ensureAlbumPicUrl(al: any): any {
  if (!al || al.picUrl) return al;
  const picUrl = ncmImageUrl(al.pic_str, al.pic);
  return picUrl ? { ...al, picUrl } : al;
}

export const transformSongData = (songs: any[], options: TransformOptions = {}) => {
  const { offset = 0, sourceId, albumTransform } = options;
  return songs.map((v, i) => {
    const rawAlbum = albumTransform ? albumTransform(v) : v.al;
    return {
      id: v.id,
      num: i + 1 + offset,
      name: v.name,
      artist: v.ar,
      album: ensureAlbumPicUrl(rawAlbum),
      alia: v.alia,
      time: getSongTime(v.dt),
      fee: v.fee,
      pc: v.pc || null,
      mv: v.mv || null,
      ...(sourceId != null ? { sourceId } : {}),
    };
  });
};
