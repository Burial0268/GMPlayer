<template>
  <!-- 全局配置组件 -->
  <n-config-provider
    :locale="zhCN"
    :date-locale="dateZhCN"
    :theme="theme"
    :theme-overrides="themeOverrides"
    :breakpoints="{
      xs: 0,
      mb: 480,
      s: 640,
      m: 1024,
      l: 1280,
      xl: 1536,
      xxl: 1920,
    }"
    abstract
    inline-theme-disabled
  >
    <n-global-style />
    <n-loading-bar-provider>
      <n-dialog-provider>
        <n-notification-provider :placement="isMobile ? 'top' : 'top-right'">
          <n-message-provider :max="3" :duration="2000" :placement="isMobile ? 'top' : 'top'">
            <slot></slot>
            <NaiveProviderContent />
          </n-message-provider>
        </n-notification-provider>
      </n-dialog-provider>
    </n-loading-bar-provider>
  </n-config-provider>
</template>

<script setup>
import {
  zhCN,
  dateZhCN,
  darkTheme,
  useOsTheme,
  useLoadingBar,
  useDialog,
  useMessage,
  useNotification,
} from "naive-ui";
import { settingStore } from "@/store";
import themeColorData from "./themeColor.json";

const setting = settingStore();

// 检测是否为移动端（宽度小于768px或存在触摸设备特征）
const isMobile = ref(false);
const checkMobile = () => {
  isMobile.value = window.innerWidth < 768 || "ontouchstart" in window;
};
checkMobile();
window.addEventListener("resize", checkMobile);
const osThemeRef = useOsTheme();
const themeOverrides = ref(null);

// 明暗切换
const theme = ref(null);
const themeColorMeta = document.querySelector('meta[name="theme-color"]');
const changeTheme = () => {
  if (setting.getSiteTheme === "light") {
    theme.value = null;
    themeColorMeta.setAttribute("content", "#ffffff");
    setCssVariable("--message-bg", "rgba(255, 255, 255, 0.72)");
    setCssVariable("--message-border", "rgba(0, 0, 0, 0.06)");
    setCssVariable("--acrylic-bg", "rgba(255, 255, 255, 0.45)");
    setCssVariable("--acrylic-border", "rgba(0, 0, 0, 0.04)");
    setCssVariable("--layout-bg", "#fff");
  } else if (setting.getSiteTheme === "dark") {
    theme.value = darkTheme;
    themeColorMeta.setAttribute("content", "#18181c");
    setCssVariable("--message-bg", "rgba(48, 48, 51, 0.72)");
    setCssVariable("--message-border", "rgba(255, 255, 255, 0.08)");
    setCssVariable("--acrylic-bg", "rgba(24, 24, 28, 0.45)");
    setCssVariable("--acrylic-border", "rgba(255, 255, 255, 0.04)");
    setCssVariable("--layout-bg", "#18181c");
  }
};

// 根据系统决定明暗切换
const osThemeChange = (val) => {
  if (setting.themeAuto) {
    setting.theme = val === "dark" ? "dark" : "light";
  }
};

// 配置主题色
const changeThemeColor = (val) => {
  let color = null;
  if (val !== "custom") {
    color = themeColorData[val];
    console.log("当前主题色：" + val, color);
    themeOverrides.value = {
      common: color,
    };
    setting.themeData = color;
  } else {
    color = setting.themeData;
    console.log("当前主题色为自定义：" + val, color);
    themeOverrides.value = {
      common: color,
    };
  }
  setCssVariable("--main-color", color.primaryColor);
  setCssVariable("--main-second-color", color.primaryColor + "1f");
  setCssVariable("--main-boxshadow-color", color.primaryColor + "26");
  setCssVariable("--main-boxshadow-hover-color", color.primaryColor + "05");
};

// 修改全局颜色
const setCssVariable = (name, value) => {
  document.documentElement.style.setProperty(name, value);
  // document.body.style.setProperty(name, value);
};

// 挂载 naive 组件的方法
const setupNaiveTools = () => {
  window.$loadingBar = useLoadingBar(); // 进度条
  window.$notification = useNotification(); // 通知
  window.$message = useMessage(); // 信息
  window.$dialog = useDialog(); // 对话框
};

const NaiveProviderContent = defineComponent({
  setup() {
    setupNaiveTools();
  },
  render() {},
});

// 监听明暗变化
watch(
  () => setting.getSiteTheme,
  () => {
    changeTheme();
  },
);

// 监听系统明暗变化
watch(
  () => osThemeRef.value,
  (val) => {
    osThemeChange(val);
  },
);

// 监听主题色变化
watch(
  () => setting.themeType,
  (val) => changeThemeColor(val),
);
watch(
  () => setting.themeData,
  (val) => changeThemeColor(val.label),
);

onMounted(() => {
  changeTheme();
  changeThemeColor(setting.themeType);
  osThemeChange(osThemeRef.value);
});
</script>
