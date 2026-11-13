<template>
  <div class="login">
    <div class="title">
      <img src="/images/logo/favicon.png" alt="logo" />
      <n-text>{{ $t("login.login", { name: siteTitle }) }}</n-text>
    </div>
    <n-tabs
      animated
      class="content"
      type="segment"
      justify-content="space-evenly"
      :pane-style="{
        display: 'flex',
        flexDirection: 'column',
        alignItems: 'center',
        paddingTop: '30px',
      }"
      @update:value="tabChange"
    >
      <n-tab-pane name="qr" :tab="$t('login.qr')">
        <n-card class="qr-img">
          <n-skeleton v-if="!qrImg" style="min-width: 180px" height="180px" width="180px" />
          <QrcodeVue
            v-else
            class="qr"
            :value="qrImg"
            :size="180"
            level="H"
            background="#00000000"
            :foreground="setting.themeData.primaryColor"
          />
        </n-card>
        <span class="tip">{{ loginStatus }}</span>
      </n-tab-pane>
      <n-tab-pane name="phone" :tab="$t('login.phone')">
        <n-form
          class="phone"
          ref="phoneFormRef"
          :model="phoneFormData"
          :rules="phoneFormRules"
          :show-label="false"
        >
          <n-form-item path="phone">
            <n-input placeholder="请输入手机号" v-model:value="phoneFormData.phone">
              <template #prefix>
                <n-icon :component="PhoneAndroidRound" />
              </template>
            </n-input>
          </n-form-item>
          <n-form-item class="captcha-item" path="captcha">
            <div class="captcha-field">
              <n-input-otp
                class="otp-input"
                v-model:value="phoneFormData.captcha"
                :length="4"
                :allow-input="allowCaptchaInput"
                placeholder=""
              />
              <n-button
                class="send-btn"
                type="primary"
                :disabled="captchaDisabled"
                @click="getCaptcha(phoneFormData.phone)"
              >
                {{ captchaText }}
              </n-button>
            </div>
          </n-form-item>
          <n-form-item>
            <n-button style="width: 100%" type="primary" @click="phoneLogin">
              {{ $t("login.login") }}
            </n-button>
          </n-form-item>
        </n-form>
      </n-tab-pane>
      <n-tab-pane name="email" :tab="$t('login.email')">
        <n-alert style="width: 100%; margin-top: -20px; margin-bottom: 12px" type="warning">
          {{ $t("login.canNotUse") }}
        </n-alert>
      </n-tab-pane>
    </n-tabs>
  </div>
</template>

<script setup lang="ts">
import { userStore, musicStore, settingStore } from "@/store";
import { getLoginState, getQrKey, checkQr, toLogin, sentCaptcha, verifyCaptcha } from "@/api/login";
import { useRoute, useRouter } from "vue-router";
import { useLayerNavigation } from "@/utils/navigation";
import type { RootEntry } from "@/utils/navigation/layers";
import { PhoneAndroidRound } from "@vicons/material";
import { formRules } from "@/utils/ui/formRules";
import { useI18n } from "vue-i18n";
import QrcodeVue from "qrcode.vue";
import { NInputOtp, type FormRules } from "naive-ui";

const { t } = useI18n();
const router = useRouter();
const route = useRoute();
const navigation = useLayerNavigation();

// Leave the way the user came: a login the guard forced continues to the page that was asked
// for, any other one goes back to the page that offered it.
const leaveLogin = () => {
  const redirect = typeof route.query.redirect === "string" ? route.query.redirect : "";
  if (!redirect) return void navigation.closeTop();
  const root = router.resolve(redirect).meta.navigationRoot as RootEntry | undefined;
  if (root) void navigation.switchRoot(root, redirect);
  else void navigation.replacePage(redirect);
};
const user = userStore();
const music = musicStore();
const setting = settingStore();
const siteTitle = import.meta.env.VITE_SITE_TITLE;
const { mobileRule } = formRules();

// 二维码数据
const qrImg = ref(null);
const loginStatus = ref(t("login.loginStatus1"));

interface PhoneFormData {
  phone: string | null;
  captcha: string[];
}

// 手机号登录数据
const phoneFormRef = ref(null);
const phoneFormData = ref<PhoneFormData>({
  phone: null,
  captcha: [],
});
const phoneFormRules: FormRules = {
  phone: mobileRule,
  captcha: {
    required: true,
    validator(_rule, value) {
      if (
        !Array.isArray(value) ||
        value.join("").length !== 4 ||
        value.some((v) => !/^\d$/.test(v))
      ) {
        return new Error("请输入短信验证码");
      }
      return true;
    },
    trigger: ["input", "blur"],
  },
};
let captchaTimeOut = null;
const captchaText = ref(t("login.getCode"));
const captchaDisabled = ref(false);

const allowCaptchaInput = (char: string) => /^\d$/.test(char);

// 二维码轮询会话
let qrCheckInterval: ReturnType<typeof setInterval> | null = null;
let qrSessionId = 0;
let qrCheckInFlight = false;
let qrTabActive = false;

// 登陆状态弹窗
const loginStateMessage = ref(null);

// 是否已卸载
let isUnmounted = false;

const clearLoginStateMessage = () => {
  loginStateMessage.value?.destroy?.();
  loginStateMessage.value = null;
};

const clearQrPollingTimer = () => {
  if (qrCheckInterval !== null) {
    clearInterval(qrCheckInterval);
    qrCheckInterval = null;
  }
  qrCheckInFlight = false;
};

// 使旧的二维码请求和轮询响应失效，避免 keep-alive 页面留下孤儿定时器
const stopQrPolling = () => {
  qrSessionId += 1;
  clearQrPollingTimer();
  clearLoginStateMessage();
};

const isQrSessionActive = (sessionId: number) =>
  !isUnmounted && qrTabActive && sessionId === qrSessionId;

const startQrPollingSession = () => {
  if (isUnmounted || !qrTabActive) return;
  stopQrPolling();
  void getQrKeyData(qrSessionId);
};

const activateLoginPage = () => {
  if (isUnmounted || qrTabActive) return;
  qrTabActive = true;
  music.setPlayBarState(false);
  startQrPollingSession();
};

// 储存登录信息
const saveLoginData = async (data, sessionId?: number) => {
  data.cookie = data.cookie.replaceAll(" HTTPOnly", "");
  user.setCookie(data.cookie);
  // 验证用户登录信息
  try {
    const res = await getLoginState();
    if (isUnmounted || (sessionId !== undefined && !isQrSessionActive(sessionId))) return;
    if (res.data.profile) {
      stopQrPolling();
      user.setUserData(res.data.profile);
      user.userLogin = true;
      loginStatus.value = t("login.loginStatus4");
      $message.success(t("login.loginStatus4"));
      // 自动签到
      if ($signIn) $signIn();
      leaveLogin();
    } else {
      user.userLogOut();
      $message.error(t("login.loginStatus5"));
      if (sessionId !== undefined) startQrPollingSession();
    }
  } catch (err) {
    console.error(err);
    if (sessionId !== undefined && isQrSessionActive(sessionId)) {
      startQrPollingSession();
    } else if (!isUnmounted) {
      $message.error(t("login.loginStatus5"));
    }
  }
};

// 获取二维码登录 key
const getQrKeyData = async (sessionId = qrSessionId) => {
  if (!isQrSessionActive(sessionId)) return;
  try {
    // 检测是否登录
    const stateRes = await getLoginState();
    if (!isQrSessionActive(sessionId)) return;
    if (stateRes.data.profile && window.localStorage.getItem("cookie")) {
      stopQrPolling();
      $message.info(t("login.loggedIn"));
      user.userLogin = true;
      leaveLogin();
    } else {
      user.userLogOut();
      clearQrPollingTimer();
      const qrRes = await getQrKey();
      if (!isQrSessionActive(sessionId)) return;
      if (qrRes.code === 200) {
        qrImg.value = `https://music.163.com/login?codekey=${qrRes.data.unikey}`;
        checkQrState(qrRes.data.unikey, sessionId);
      } else {
        $message.error(t("login.loginStatus6"));
      }
    }
  } catch (err) {
    console.error(err);
  }
};

// 检测二维码登陆状态
const checkQrState = (key, sessionId: number) => {
  if (!isQrSessionActive(sessionId)) return;
  clearQrPollingTimer();
  qrCheckInterval = setInterval(() => {
    if (!key || !isQrSessionActive(sessionId) || qrCheckInFlight) return;
    qrCheckInFlight = true;
    checkQr(key)
      .then((res) => {
        if (!isQrSessionActive(sessionId)) return;
        if (res.code === 800) {
          stopQrPolling();
          loginStatus.value = t("login.loginStatus2");
          startQrPollingSession();
        } else if (res.code === 801) {
          clearLoginStateMessage();
          loginStatus.value = t("login.loginStatus1");
        } else if (res.code === 802) {
          loginStatus.value = t("login.loginStatus3");
          if (!loginStateMessage.value) {
            loginStateMessage.value = $message.loading(t("login.loginStatus3"), {
              duration: 0,
            });
          }
        } else if (res.code === 803) {
          clearQrPollingTimer();
          clearLoginStateMessage();
          saveLoginData(res, sessionId);
        }
      })
      .catch((err) => {
        if (isQrSessionActive(sessionId)) console.error(err);
      })
      .finally(() => {
        if (sessionId === qrSessionId) qrCheckInFlight = false;
      });
  }, 1000);
};

// 获取验证码
const getCaptcha = (data) => {
  clearInterval(captchaTimeOut);
  phoneFormRef.value?.validate(
    async (errors) => {
      if (errors) {
        $message.error(t("general.message.needCheck"));
      } else {
        captchaDisabled.value = true;
        try {
          const res = await sentCaptcha(data);
          if (isUnmounted) return;
          if (res.code === 200) {
            $message.success(t("login.codeSuccess"));
            let countDown = 60;
            captchaTimeOut = setInterval(() => {
              countDown--;
              captchaText.value = countDown + "s";
              if (countDown === 0) {
                clearInterval(captchaTimeOut);
                captchaText.value = t("login.getCodeAgain");
                captchaDisabled.value = false;
              }
            }, 1000);
          } else {
            captchaDisabled.value = false;
            $message.error(t("login.codeError"));
          }
        } catch (err) {
          console.error(err);
          if (isUnmounted) return;
          captchaDisabled.value = false;
          captchaText.value = t("login.getCodeAgain");
          $message.error(t("login.codeError"));
        }
      }
    },
    (rule) => {
      return rule?.key === "phone";
    },
  );
};

// 手机号登录
const phoneLogin = async (e) => {
  e.preventDefault();
  phoneFormRef.value?.validate(async (errors) => {
    if (!errors) {
      try {
        const captcha = phoneFormData.value.captcha.join("");
        const verifyRes = await verifyCaptcha(phoneFormData.value.phone, captcha);
        if (isUnmounted) return;
        if (verifyRes.code === 200) {
          const loginRes = await toLogin(phoneFormData.value.phone, captcha);
          if (isUnmounted) return;
          if (loginRes.profile) {
            await saveLoginData(loginRes);
          } else {
            user.userLogOut();
            $message.error(t("login.loginStatus5"));
            phoneFormData.value.captcha = [];
          }
        }
      } catch (err) {
        console.error(err);
        $loadingBar.error();
        $message.error(t("login.loginStatus5"));
      }
    } else {
      $loadingBar.error();
      $message.error(t("general.message.needCheck"));
    }
  });
};

// Tab 切换
const tabChange = (val) => {
  if (val === "qr") {
    startQrPollingSession();
  } else {
    stopQrPolling();
  }
};

onMounted(() => {
  $setSiteTitle(t("login.login"));
  activateLoginPage();
});

onActivated(() => {
  // keep-alive 复用时重新获取二维码；首次激活由 onMounted 的幂等保护接管
  activateLoginPage();
});

onDeactivated(() => {
  qrTabActive = false;
  // keep-alive 缓存时恢复控制条并清除定时器
  music.setPlayBarState(true);
  stopQrPolling();
  clearInterval(captchaTimeOut);
});

onBeforeUnmount(() => {
  isUnmounted = true;
  qrTabActive = false;
  // 恢复控制条
  music.setPlayBarState(true);
  // 清除定时器
  stopQrPolling();
  clearInterval(captchaTimeOut);
});
</script>

<style lang="scss" scoped>
.login {
  margin-top: 40px;
  display: flex;
  flex-direction: column;
  align-items: center;
  .title {
    display: flex;
    flex-direction: column;
    align-items: center;
    img {
      width: 80px;
      height: 80px;
      margin-bottom: 20px;
    }
    span {
      font-size: 26px;
      font-weight: bold;
    }
  }
  .content {
    width: 300px;
    margin-top: 30px;
    .qr-img {
      width: 220px;
      height: 220px;
      border-radius: var(--radius-md);
      background-color: #fff;
      :deep(.n-card__content) {
        display: flex;
        align-items: center;
        justify-content: center;
        .n-skeleton {
          border-radius: var(--radius-md);
        }
      }
    }
    .tip {
      margin: 12px 0;
      opacity: 0.8;
    }
    .phone {
      width: 100%;
      padding: 0 4px;
      box-sizing: border-box;
      .captcha-field {
        width: 100%;
        display: flex;
        flex-direction: row;
        align-items: center;
        gap: 12px;
        .otp-input {
          flex-shrink: 0;
        }
        .send-btn {
          flex: 1;
          white-space: nowrap;
        }
      }
    }
    :deep(.n-input) {
      .n-input__prefix {
        margin-right: 8px;
      }
    }
  }
}
</style>
