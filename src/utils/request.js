import axios from "axios";
let baseURL = "";

switch (process.env.NODE_ENV) {
  case "production":
    // 生产环境也使用 '/api'，需要在部署服务器 (Nginx、Apache、Netlify 等) 上做反向代理到真实后端，规避 CORS
    baseURL = import.meta.env.VITE_MUSIC_API;
    break;
  case "development":
    baseURL = "/api/ncm";
    break;
  default:
    baseURL = "/api/ncm";
    break;
}

axios.defaults.baseURL = baseURL;
axios.defaults.timeout = 30000;
axios.defaults.headers.common["X-Requested-With"] = "XMLHttpRequest";
axios.defaults.withCredentials = true;

// 请求拦截
axios.interceptors.request.use(
  (request) => {
    // 检查是否为 Lyric Atlas API 的请求
    const isLyricAtlasRequest = request.url && request.url.startsWith('/api/la');

    // Ensure headers object exists
    request.headers = request.headers || {};

    if (isLyricAtlasRequest) {
      // 对 Lyric Atlas API：
      // 1. 禁用凭据
      request.withCredentials = false;
      // 2. 移除 X-Requested-With 请求头
      delete request.headers['X-Requested-With'];
      // 3. 覆盖默认的 baseURL，确保请求根路径正确
      request.baseURL = '/';
      console.log('[Axios Interceptor] Lyric Atlas request - Overrode baseURL, removed X-Requested-With, set withCredentials=false');
    } else {
      // 对其他请求：
      // 1. 确保使用默认的 baseURL
      request.baseURL = baseURL;
      // 2. 确保凭据设置为 true
      request.withCredentials = true;
      // 3. 确保 X-Requested-With 请求头存在
      if (!request.headers['X-Requested-With']) {
          request.headers['X-Requested-With'] = 'XMLHttpRequest';
      }
      console.log('[Axios Interceptor] Other request - Ensured baseURL, X-Requested-With, set withCredentials=true');
    }

    if (!request.hiddenBar && typeof $loadingBar !== "undefined")
      $loadingBar.start();
    return request;
  },
  (error) => {
    if (typeof $loadingBar !== "undefined") $loadingBar.error(); // Ensure loading bar handles error
    console.error("请求失败，请稍后重试");
    return Promise.reject(error);
  }
);

// 响应拦截
axios.interceptors.response.use(
  (response) => {
    if (typeof $loadingBar !== "undefined") $loadingBar.finish();
    return response.data;
  },
  (error) => {
    if (typeof $loadingBar !== "undefined") $loadingBar.error(); // Ensure loading bar handles error
    if (error.response) {
      const data = error.response.data;
      switch (error.response.status) {
        case 401:
          console.error(data.message ? data.message : "无权限访问");
          break;
        case 301:
          console.error(data.message ? data.message : "请求发生重定向");
          break;
        case 404:
          console.error(data.message ? data.message : "请求资源不存在");
          break;
        case 500:
          console.error(data.message ? data.message : "内部服务器错误");
          break;
        default:
          console.error(data.message ? data.message : "请求失败，请稍后重试");
          break;
      }
    } else {
      console.error("请求失败，请稍后重试");
    }
    return Promise.reject(error);
  }
);

export default axios; 