/**
 * Login API - 登录相关接口
 */

import request from "@/utils/request";

export const login = {
  /**
   * 生成二维码 key
   */
  getQrKey: () =>
    request<any>({
      method: "GET",
      hiddenBar: true,
      url: "/login/qr/key",
      params: { timestamp: Date.now() },
    }),

  /**
   * 生成二维码
   */
  createQr: (key: string, qrimg = true) =>
    request<any>({
      method: "GET",
      hiddenBar: true,
      url: "/login/qr/create",
      params: { key, qrimg, timestamp: Date.now() },
    }),

  /**
   * 检查二维码状态
   */
  checkQr: (key: string) =>
    request<any>({
      method: "GET",
      hiddenBar: true,
      url: "/login/qr/check",
      params: { key, timestamp: Date.now() },
    }),

  /**
   * 手机号登录
   */
  byPhone: (phone: string, captcha: string) =>
    request<any>({
      method: "GET",
      hiddenBar: true,
      url: "/login/cellphone",
      params: { phone, captcha, timestamp: Date.now() },
    }),

  /**
   * 发送验证码
   */
  sendCaptcha: (phone: string) =>
    request<any>({
      method: "GET",
      url: "/captcha/sent",
      params: { phone, timestamp: Date.now() },
    }),

  /**
   * 验证验证码
   */
  verifyCaptcha: (phone: string, captcha: string) =>
    request<any>({
      method: "GET",
      hiddenBar: true,
      url: "/captcha/verify",
      params: { phone, captcha, timestamp: Date.now() },
    }),

  /**
   * 获取登录状态
   */
  getStatus: () =>
    request<any>({
      method: "GET",
      hiddenBar: true,
      url: "/login/status",
      params: { timestamp: Date.now() },
    }),

  /**
   * 刷新登录
   */
  refresh: () =>
    request<any>({
      method: "GET",
      hiddenBar: true,
      url: "/login/refresh",
      params: { timestamp: Date.now() },
    }),

  /**
   * 退出登录
   */
  logout: () =>
    request<any>({
      method: "GET",
      url: "/logout",
      params: { timestamp: Date.now() },
    }),
};

export default login;

// Legacy exports
export const getQrKey = login.getQrKey;
export const qrCreate = login.createQr;
export const checkQr = login.checkQr;
export const toLogin = login.byPhone;
export const sentCaptcha = login.sendCaptcha;
export const verifyCaptcha = login.verifyCaptcha;
export const getLoginState = login.getStatus;
export const refreshLogin = login.refresh;
export const userLogOut = login.logout;
