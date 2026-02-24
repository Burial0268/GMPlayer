import { Md5 } from "ts-md5";

const MAGIC_KEY = "3go8&$8*3*3h0k(2)2";

const MAGIC_BYTES: Uint8Array = (() => {
  const buf = new Uint8Array(MAGIC_KEY.length);
  for (let i = 0; i < MAGIC_KEY.length; i++) {
    buf[i] = MAGIC_KEY.charCodeAt(i);
  }
  return buf;
})();

const _md5 = new Md5();

function encryptId(idStr: string): string {
  const len = idStr.length;
  const xored = new Uint8Array(len);
  const mlen = MAGIC_BYTES.length;

  for (let i = 0; i < len; i++) {
    xored[i] = idStr.charCodeAt(i) ^ MAGIC_BYTES[i % mlen];
  }

  _md5.start(); // reset
  _md5.appendByteArray(xored);
  const bytes = new Uint8Array((_md5.end(true) as Int32Array).buffer);

  // 一次展开避免循环拼接
  return btoa(String.fromCharCode(...bytes))
    .replace(/[/+]/g, (c) => (c === "/" ? "_" : "-"));
}

/**
 * 一个通用组件，用以将 NCM Album/Artist API 获取到的 pic_str/pic ID 转化为实际图像 URL
 *
 * @param picStr 返回结构体中的 pic_str
 * @param pic 返回结构体中的 pic
 * @returns 处理后的歌词行数组
 * @example  // 这是一个上文所示的返回结构体中的标准参照
 *    "al": {
        "id": 148811451,
        "name": "Everything Goes On",
        "pic_str": "109951171796454922",
        "pic": 109951171796454930
      }
 */
export function ncmImageUrl(picStr?: string, pic?: number): string | undefined {
  const id = picStr ?? (pic != null ? String(pic) : "");
  if (!id) return undefined;
  return `https://p1.music.126.net/${encryptId(id)}/${id}.jpg`;
}