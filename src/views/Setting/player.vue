<template>
  <div class="set-player">
    <n-card class="set-item">
      <div class="name">
        {{ $t("setting.playerStyle") }}
        <span class="tip">{{ $t("setting.playerStyleTip") }}</span>
      </div>
      <n-select class="set" v-model:value="playerStyle" :options="playerStyleOptions" />
    </n-card>
    <n-card class="set-item">
      <div class="name">
        {{ $t("setting.backgroundImageShow") }}
        <span class="tip">{{ $t(`setting.backgroundImageShowTip_${backgroundImageShow}`) }}</span>
      </div>
      <n-select
        class="set"
        v-model:value="backgroundImageShow"
        :options="backgroundImageShowOptions"
      />
    </n-card>
    <n-card v-if="backgroundImageShow === 'eplor'" class="set-item">
      <div class="name">
        {{ $t("setting.eploryBackgroundConfig") }}
        <span class="tip">{{ $t(`setting.eploryBackgroundConfigTip`) }}</span>
      </div>
      <n-button class="set" @click="isModalOn = true">配置</n-button>
      <n-modal
        class="s-modal"
        preset="dialog"
        :title="t('setting.eploryBackgroundConfig')"
        v-model:show="isModalOn"
      >
        <n-form style="margin-top: 1rem">
          <n-form-item :label="t('setting.eplorySetting.fps.title')">
            <n-input-number :min="0.1" v-model:value="fps" />
            <template #feedback>
              {{ t("setting.eplorySetting.fps.tip") }}
            </template>
          </n-form-item>
          <br />
          <n-form-item label="启用动态流速">
            <n-switch v-model:value="dynamicFlowSpeed" />
            <template #feedback>
              启用动态流速，滚动背景会根据音乐当前的烈度来调整流速，以实现更加灵动的显示效果。同时，普通的流速设置将被禁用。
              <br /><br />
              使用该功能则将伴随启用音乐 FFT 分析，这会导致较大的性能损耗，请根据设备能力选择。
            </template>
          </n-form-item>
          <br />
          <n-form-item label="动态流速倍率">
            <n-input-number
              :min="0.1"
              v-model:value="dynamicFlowSpeedScale"
              :disabled="!dynamicFlowSpeed"
            />
            <template #feedback>
              调节动态流速的乘算倍率，倍率越高，随音频烈度的而产生的流速变化会越明显，性能消耗也会越大，默认为
              2。
            </template>
          </n-form-item>
          <br />
          <n-form-item :label="t('setting.eplorySetting.flowSpeed.title')">
            <n-input-number :min="0.1" v-model:value="flowSpeed" :disabled="dynamicFlowSpeed" />
            <template #feedback>
              {{ t("setting.eplorySetting.flowSpeed.tip") }}
            </template>
          </n-form-item>
          <br />
          <n-form-item :label="t('setting.eplorySetting.renderScale.title')">
            <n-input-number :min="0.1" v-model:value="renderScale" />
            <template #feedback>
              {{ t("setting.eplorySetting.renderScale.tip") }}
            </template>
          </n-form-item>
          <br />
          <n-form-item :label="t('setting.eplorySetting.albumImageUrl.title')">
            <n-input v-model:value="albumImageUrl" />
            <template #feedback>
              {{ t("setting.eplorySetting.albumImageUrl.tip") }}
            </template>
          </n-form-item>
        </n-form>
      </n-modal>
    </n-card>
    <n-card v-if="backgroundImageShow === 'blur'" class="set-item">
      <div class="name">
        WebGL 模糊效果设置
        <span class="tip">使用 WebGL 渲染的高性能模糊效果，降低设备性能消耗</span>
      </div>
      <n-button class="set" @click="isBlurModalOn = true">配置</n-button>
      <n-modal
        class="s-modal"
        preset="dialog"
        title="WebGL 模糊效果设置"
        v-model:show="isBlurModalOn"
      >
        <n-form style="margin-top: 1rem">
          <n-form-item label="FPS">
            <n-input-number :min="1" :max="60" v-model:value="fps" />
            <template #feedback> 设置渲染帧率，值越低性能消耗越小，但动画可能不够流畅 </template>
          </n-form-item>
          <br />
          <n-form-item label="模糊程度">
            <n-input-number :min="1" :max="100" v-model:value="blurAmount" />
            <template #feedback> 设置背景模糊的强度，值越大模糊效果越强 </template>
          </n-form-item>
          <br />
          <n-form-item label="对比度">
            <n-input-number :min="0.1" :max="3" :step="0.1" v-model:value="contrastAmount" />
            <template #feedback> 设置背景的对比度，提高对比度可以使色彩更加鲜明 </template>
          </n-form-item>
          <br />
          <n-form-item label="渲染比例">
            <n-input-number :min="0.1" :max="1" :step="0.1" v-model:value="renderScale" />
            <template #feedback> 设置渲染的画布比例，值越低性能消耗越小，但画质可能降低 </template>
          </n-form-item>
        </n-form>
      </n-modal>
    </n-card>
    <n-card class="set-item">
      <div class="name">
        {{ $t("setting.showTransl") }}
        <span class="tip">{{ $t("setting.showTranslTip") }}</span>
      </div>
      <n-switch v-model:value="showTransl" :round="false" />
    </n-card>
    <n-card class="set-item">
      <div class="name">
        {{ $t("setting.showRoma") }}
        <span class="tip">{{ $t("setting.showRomaTip") }}</span>
      </div>
      <n-switch v-model:value="showRoma" :round="false" />
    </n-card>
    <n-card class="set-item">
      <div class="name">
        <div class="dev">
          使用 AMLL TTML 歌词仓库
          <n-tag round :bordered="false" size="small" type="warning">
            {{ $t("setting.dev") }}
            <template #icon>
              <n-icon :component="Code" />
            </template>
          </n-tag>
        </div>
        <span class="tip"
          >优先从 AMLL TTML
          歌词仓库获取高精度逐字歌词（含对唱、背景行信息）；若获取失败，则回退到网易云歌词</span
        >
      </div>
      <n-switch v-model:value="useTTMLRepo" :round="false" />
    </n-card>
    <n-card class="set-item">
      <div class="name">
        <div class="dev">
          {{ $t("setting.showYrc") }}
          <n-tag round :bordered="false" size="small" type="warning">
            {{ $t("setting.dev") }}
            <template #icon>
              <n-icon :component="Code" />
            </template>
          </n-tag>
        </div>
        <span class="tip">{{ $t("setting.showYrcTip") }}</span>
      </div>
      <n-switch v-model:value="showYrc" :round="false" />
    </n-card>
    <template v-if="showYrc">
      <n-card class="set-item">
        <div class="name">
          {{ $t("setting.showYrcAnimation") }}
          <span class="tip">{{ $t("setting.showYrcAnimationTip") }}</span>
        </div>
        <n-switch v-model:value="showYrcAnimation" :round="false" />
      </n-card>
      <n-card class="set-item">
        <div class="name">
          弹簧动画参数
          <span class="tip">调整歌词动画的弹性效果</span>
        </div>
        <n-collapse>
          <n-collapse-item title="横向移动">
            <n-form-item label="质量">
              <n-input-number v-model:value="springParams.posX.mass" :min="0.1" :step="0.1" />
            </n-form-item>
            <n-form-item label="阻尼">
              <n-input-number v-model:value="springParams.posX.damping" :min="0" :step="1" />
            </n-form-item>
            <n-form-item label="刚度">
              <n-input-number v-model:value="springParams.posX.stiffness" :min="0" :step="1" />
            </n-form-item>
          </n-collapse-item>
          <n-collapse-item title="纵向移动">
            <n-form-item label="质量">
              <n-input-number v-model:value="springParams.posY.mass" :min="0.1" :step="0.1" />
            </n-form-item>
            <n-form-item label="阻尼">
              <n-input-number v-model:value="springParams.posY.damping" :min="0" :step="1" />
            </n-form-item>
            <n-form-item label="刚度">
              <n-input-number v-model:value="springParams.posY.stiffness" :min="0" :step="1" />
            </n-form-item>
          </n-collapse-item>
          <n-collapse-item title="缩放">
            <n-form-item label="质量">
              <n-input-number v-model:value="springParams.scale.mass" :min="0.1" :step="0.1" />
            </n-form-item>
            <n-form-item label="阻尼">
              <n-input-number v-model:value="springParams.scale.damping" :min="0" :step="1" />
            </n-form-item>
            <n-form-item label="刚度">
              <n-input-number v-model:value="springParams.scale.stiffness" :min="0" :step="1" />
            </n-form-item>
          </n-collapse-item>
        </n-collapse>
      </n-card>
    </template>
    <n-card class="set-item">
      <div class="name">
        {{ $t("setting.countDownShow") }}
        <span class="tip">{{ $t("setting.countDownShowTip") }}</span>
      </div>
      <n-switch v-model:value="countDownShow" :round="false" />
    </n-card>
    <n-card class="set-item">
      <div class="name">
        {{ $t("setting.lrcMousePause") }}
        <span class="tip">{{ $t("setting.lrcMousePauseTip") }}</span>
      </div>
      <n-switch v-model:value="lrcMousePause" :round="false" />
    </n-card>
    <n-card class="set-item">
      <div class="name">
        {{ $t("setting.lyricsBlock") }}
        <span class="tip">{{ $t("setting.lyricsBlockTip") }}</span>
      </div>
      <n-select class="set" v-model:value="lyricsBlock" :options="lyricsBlockOptions" />
    </n-card>
    <n-card class="set-item">
      <div class="name">
        <div class="dev">
          沉浸式播放器
          <n-tag round :bordered="false" size="small" type="warning">
            {{ $t("setting.dev") }}
            <template #icon>
              <n-icon :component="Code" />
            </template>
          </n-tag>
        </div>
        <span class="tip">使用动态取色算法，使播放器中的文字将会与封面同色</span>
      </div>
      <n-switch v-model:value="immersivePlayer" :round="false" />
    </n-card>
    <n-card class="set-item" v-if="immersivePlayer">
      <div class="name">
        <div class="dev">
          沉浸式播放器取色类别
          <n-tag round :bordered="false" size="small" type="warning">
            {{ $t("setting.dev") }}
            <template #icon>
              <n-icon :component="Code" />
            </template>
          </n-tag>
        </div>
        <span class="tip">调节取色所使用的类型</span>
      </div>
      <n-select
        class="set"
        v-model:value="colorType"
        :options="colorTypeOptions"
        :disabled="!immersivePlayer"
      />
    </n-card>
    <n-card
      class="set-item"
      :content-style="{
        flexDirection: 'column',
        alignItems: 'flex-start',
      }"
    >
      <div class="name">{{ $t("setting.lyricsFontSize") }}</div>
      <n-slider
        v-model:value="lyricsFontSize"
        :tooltip="false"
        :max="4"
        :min="3"
        :step="0.01"
        :marks="{
          3: t('setting.lyrics1'),
          3.6: t('setting.lyrics2'),
          4: t('setting.lyrics3'),
        }"
      />
    </n-card>
    <n-card class="set-item">
      <div class="name">歌词字体</div>
      <n-select
        class="set"
        v-model:value="lyricFont"
        :options="[
          { label: 'HarmonyOS Sans SC', value: 'HarmonyOS Sans SC' },
          { label: 'PingFang SC', value: 'PingFang SC' },
          { label: 'Microsoft YaHei', value: 'Microsoft YaHei' },
          { label: 'Noto Sans SC', value: 'Noto Sans SC' },
          { label: 'SF Pro Display', value: 'SF Pro Display' },
        ]"
      />
    </n-card>
    <n-card class="set-item">
      <div class="name">歌词字重</div>
      <n-select
        class="set"
        v-model:value="lyricFontWeight"
        :options="[
          { label: '常规', value: 'normal' },
          { label: '中等', value: '500' },
          { label: '粗体', value: 'bold' },
        ]"
      />
    </n-card>
    <n-card class="set-item">
      <div class="name">歌词字间距</div>
      <n-select
        class="set"
        v-model:value="lyricLetterSpacing"
        :options="[
          { label: '正常', value: 'normal' },
          { label: '紧凑', value: '-0.05em' },
          { label: '宽松', value: '0.05em' },
        ]"
      />
    </n-card>
    <n-card class="set-item">
      <div class="name">歌词行高</div>
      <n-input-number v-model:value="lyricLineHeight" :min="1" :max="3" :step="0.1" />
    </n-card>
    <n-card class="set-item">
      <div class="name">{{ $t("setting.lyricsPosition") }}</div>
      <n-select class="set" v-model:value="lyricsPosition" :options="lyricsPositionOptions" />
    </n-card>
    <n-card class="set-item">
      <div class="name">
        {{ $t("setting.lyricsBlur") }}
        <span class="tip">{{ $t("setting.lyricsBlurTip") }}</span>
      </div>
      <n-switch v-model:value="lyricsBlur" :round="false" />
    </n-card>
    <n-card class="set-item">
      <div class="name">
        显示音乐频谱
        <span class="tip">可能会消耗性能</span>
      </div>
      <n-switch v-model:value="musicFrequency" :round="false" />
    </n-card>
    <!-- AutoMix Settings -->
    <n-card class="set-item">
      <div class="name">
        <div class="dev">
          {{ $t("setting.autoMixEnabled") }}
          <n-tag round :bordered="false" size="small" type="warning">
            {{ $t("setting.dev") }}
            <template #icon>
              <n-icon :component="Code" />
            </template>
          </n-tag>
        </div>
        <span class="tip">{{ $t("setting.autoMixEnabledTip") }}</span>
      </div>
      <n-switch v-model:value="autoMixEnabled" :round="false" />
    </n-card>
    <template v-if="autoMixEnabled">
      <n-card class="set-item">
        <div class="name">
          {{ $t("setting.autoMixCrossfadeDuration") }}
          <span class="tip">{{ $t("setting.autoMixCrossfadeDurationTip") }}</span>
        </div>
        <n-input-number
          class="set"
          v-model:value="autoMixCrossfadeDuration"
          :min="3"
          :max="12"
          :step="1"
        />
      </n-card>
      <n-card class="set-item">
        <div class="name">
          {{ $t("setting.autoMixTransitionStyle") }}
          <span class="tip">{{ $t("setting.autoMixTransitionStyleTip") }}</span>
        </div>
        <n-select
          class="set"
          v-model:value="autoMixTransitionStyle"
          :options="autoMixTransitionStyleOptions"
        />
      </n-card>
      <n-card class="set-item">
        <div class="name">
          {{ $t("setting.autoMixSmartCurve") }}
          <span class="tip">{{ $t("setting.autoMixSmartCurveTip") }}</span>
        </div>
        <n-switch v-model:value="autoMixSmartCurve" :round="false" />
      </n-card>
      <n-card class="set-item">
        <div class="name">
          {{ $t("setting.autoMixVolumeNorm") }}
          <span class="tip">{{ $t("setting.autoMixVolumeNormTip") }}</span>
        </div>
        <n-switch v-model:value="autoMixVolumeNorm" :round="false" />
      </n-card>
      <n-card class="set-item">
        <div class="name">
          {{ $t("setting.autoMixBpmMatch") }}
          <span class="tip">{{ $t("setting.autoMixBpmMatchTip") }}</span>
        </div>
        <n-switch v-model:value="autoMixBpmMatch" :round="false" />
      </n-card>
      <n-card class="set-item">
        <div class="name">
          {{ $t("setting.autoMixBeatAlign") }}
          <span class="tip">{{ $t("setting.autoMixBeatAlignTip") }}</span>
        </div>
        <n-switch v-model:value="autoMixBeatAlign" :round="false" :disabled="!autoMixBpmMatch" />
      </n-card>
      <n-card class="set-item">
        <div class="name">
          {{ $t("setting.autoMixTransitionEffects") }}
          <span class="tip">{{ $t("setting.autoMixTransitionEffectsTip") }}</span>
        </div>
        <n-switch v-model:value="autoMixTransitionEffects" :round="false" />
      </n-card>
      <n-card class="set-item">
        <div class="name">
          {{ $t("setting.autoMixVocalGuard") }}
          <span class="tip">{{ $t("setting.autoMixVocalGuardTip") }}</span>
        </div>
        <n-switch v-model:value="autoMixVocalGuard" :round="false" />
      </n-card>
    </template>
  </div>
</template>

<script setup lang="ts">
import { storeToRefs } from "pinia";
import { settingStore } from "@/store";
import { useI18n } from "vue-i18n";
import { Code, Help } from "@icon-park/vue-next";
import { watch } from "vue";

const { t } = useI18n();

const setting = settingStore();
const {
  showTransl,
  lyricsPosition,
  playerStyle,
  fps,
  flowSpeed,
  immersivePlayer,
  musicFrequency,
  renderScale,
  albumImageUrl,
  dynamicFlowSpeed,
  dynamicFlowSpeedScale,
  lyricsFontSize,
  lyricsBlock,
  lyricsBlur,
  lrcMousePause,
  showYrc,
  showRoma,
  backgroundImageShow,
  countDownShow,
  showYrcAnimation,
  colorType,
  springParams,
  lyricFont,
  lyricFontWeight,
  lyricLetterSpacing,
  lyricLineHeight,
  useTTMLRepo,
  blurAmount,
  contrastAmount,
  autoMixEnabled,
  autoMixCrossfadeDuration,
  autoMixBpmMatch,
  autoMixBeatAlign,
  autoMixVolumeNorm,
  autoMixTransitionStyle,
  autoMixSmartCurve,
  autoMixTransitionEffects,
  autoMixVocalGuard,
} = storeToRefs(setting);
console.log("SETTING", fps);
const isModalOn = ref(false);
const isBlurModalOn = ref(false);

// 监听 TTML 仓库歌词设置变化
watch(useTTMLRepo, (newValue, oldValue) => {
  console.log(`[Setting] useTTMLRepo changed from ${oldValue} to ${newValue}`);
});

// 歌词位置
const lyricsPositionOptions = [
  {
    label: t("setting.positionLeft"),
    value: "left",
  },
  {
    label: t("setting.positionCenter"),
    value: "center",
  },
];

// 歌词滚动位置
const lyricsBlockOptions = [
  {
    label: t("setting.blockStart"),
    value: "start",
  },
  {
    label: t("setting.blockCenter"),
    value: "center",
  },
];

// 取色类别
const colorTypeOptions = [
  {
    label: "中性",
    value: "neutral",
  },
  {
    label: "中性变体",
    value: "neutralVariant",
  },
  {
    label: "主要",
    value: "primary",
  },
  {
    label: "次要",
    value: "secondary",
  },
  {
    label: "次次要",
    value: "tertiary",
  },
];

// 播放器样式
const playerStyleOptions = [
  {
    label: t("setting.cover"),
    value: "cover",
  },
  {
    label: t("setting.record"),
    value: "record",
  },
];

// 播放背景类型
const backgroundImageShowOptions = [
  {
    label: t("setting.solid"),
    value: "solid",
  },
  {
    label: t("setting.blur"),
    value: "blur",
  },
  {
    label: t("setting.eplor"),
    value: "eplor",
  },
];

// AutoMix 过渡曲线类型
const autoMixTransitionStyleOptions = [
  {
    label: t("setting.autoMixEqualPower"),
    value: "equalPower",
  },
  {
    label: t("setting.autoMixLinear"),
    value: "linear",
  },
  {
    label: t("setting.autoMixSCurve"),
    value: "sCurve",
  },
];
</script>
