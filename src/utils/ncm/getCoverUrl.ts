import { snapCoverSize } from "./coverUrl";

/**
 * 获取图片的 url
 * @param url - 必选参数，输入的原始图片url
 * @param size - 可选参数，需要生成的图片尺寸，默认为null
 * @return 返回根据 url 和 size 参数生成的图片url
 */
const getCoverUrl = (url: string, size: number | null = null): string => {
  if (!url) return "/images/pic/default.png";
  const snapped = snapCoverSize(size || 50);
  const sizeUrl = `?param=${snapped}y${snapped}`;
  const imageUrl = url.replace(/^http:/, "https:");
  if (imageUrl.endsWith(".jpg")) {
    return imageUrl + sizeUrl;
  }
  return imageUrl;
};

export default getCoverUrl;
