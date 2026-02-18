import { Md5 } from "ts-md5";

const MAGIC_KEY = "3go8&$8*3*3h0k(2)2";

/**
 * 网易云音乐图片 ID 加密算法
 * 将 pic_str/dfsId 转换为 URL 中的 encrypted_id 部分
 */
function encryptId(idStr: string): string {
  const xored = new Uint8Array(idStr.length);
  for (let i = 0; i < idStr.length; i++) {
    xored[i] = idStr.charCodeAt(i) ^ MAGIC_KEY.charCodeAt(i % MAGIC_KEY.length);
  }
  const md5 = new Md5();
  md5.appendByteArray(xored);
  // end(true) 返回 Int32Array(4)，即 16 字节原始 MD5（little-endian）
  const bytes = new Uint8Array((md5.end(true) as Int32Array).buffer);
  let binary = "";
  for (let i = 0; i < bytes.length; i++) {
    binary += String.fromCharCode(bytes[i]);
  }
  return btoa(binary).replace(/\//g, "_").replace(/\+/g, "-");
}

/**
 * 从 pic_str 或 pic (number) 构造网易云图片 URL
 * @param picStr - 图片 ID 字符串 (优先使用，无精度损失)
 * @param pic - 图片 ID 数字 (JS 大整数可能有精度损失，仅作 fallback)
 */
export function ncmImageUrl(picStr?: string, pic?: number): string | undefined {
  const id = picStr || (pic ? String(pic) : "");
  if (!id) return undefined;
  return `https://p1.music.126.net/${encryptId(id)}/${id}.jpg`;
}
