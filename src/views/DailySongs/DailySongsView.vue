<template>
  <div class="dailysongs">
    <!-- 与评论页、用户页同一套头部：方块（这里是日期）+ 小标签 + 大标题，右侧放操作。 -->
    <header class="daily-head">
      <div class="date-tile" aria-hidden="true">
        <n-icon class="calendar" :component="CalendarTodayFilled" />
        <span class="day">{{ displayDay }}</span>
      </div>
      <div class="text">
        <span class="eyebrow s-label">{{ displayDate }}</span>
        <h1 class="title">{{ $t("home.modules.dailySongs.title") }}</h1>
        <n-text class="tip" :depth="3">{{ displaySubtitle }}</n-text>
      </div>
      <n-select
        v-model:value="selectedDate"
        class="history-select"
        size="small"
        filterable
        :aria-label="$t('home.modules.dailySongs.historySelect')"
        :placeholder="$t('home.modules.dailySongs.historySelect')"
        :loading="historyDatesLoading"
        :disabled="songsLoading"
        :options="historyDateOptions"
        @update:value="handleDateChange"
      />
    </header>
    <DataLists
      :listData="displaySongs"
      :loading="songsLoading"
      virtual
      virtual-height="min(68vh, 760px)"
      :virtual-item-size="82"
      :virtual-threshold="40"
    />
  </div>
</template>

<script setup lang="ts">
import { getDailySongs, getDailySongsHistory, getDailySongsHistoryDetail } from "@/api/home";
import { musicStore } from "@/store";
import { transformSongData } from "@/utils/ncm/transformSongData";
import { getDailySongsDate } from "@/utils/timeTools";
import { CalendarTodayFilled } from "@vicons/material";
import { useI18n } from "vue-i18n";
import DataLists from "@/components/DataList/DataLists.vue";

const { t } = useI18n();
const music = musicStore();
const todayValue = "today";
const selectedDate = ref(todayValue);
const historyDates = ref<string[]>([]);
const historySongsCache = ref<Record<string, any[]>>({});
const historyDatesLoading = ref(false);
const songsLoading = ref(false);
const todayDate = computed(() => getDailySongsDate());

const displaySongs = computed(() => {
  if (selectedDate.value === todayValue) return music.getDailySongs;
  return historySongsCache.value[selectedDate.value] ?? [];
});

const displayDay = computed(() => {
  if (selectedDate.value === todayValue) return Number(todayDate.value.split("-")[2]);
  return Number(selectedDate.value.split("-")[2]) || new Date().getDate();
});

const formatHistoryDate = (date: string) => {
  const [year, month, day] = date.split("-");
  if (!year || !month || !day) return date;
  return t("home.modules.dailySongs.historyDate", {
    year,
    month: Number(month),
    day: Number(day),
  });
};

const displayDate = computed(() =>
  formatHistoryDate(selectedDate.value === todayValue ? todayDate.value : selectedDate.value),
);

const displaySubtitle = computed(() => {
  if (selectedDate.value === todayValue) return t("home.modules.dailySongs.subtitle");
  return t("home.modules.dailySongs.historySubtitle", {
    date: formatHistoryDate(selectedDate.value),
  });
});

const historyDateOptions = computed(() => [
  {
    label: t("home.modules.dailySongs.today"),
    value: todayValue,
  },
  ...historyDates.value
    .filter((date) => date !== todayDate.value)
    .map((date) => ({
      label: formatHistoryDate(date),
      value: date,
    })),
]);

const extractDailySongs = (res: any) => res.data?.dailySongs ?? res.data?.songs ?? [];

// 获取每日推荐数据
const getDailySongsData = async () => {
  const dailySongsDate = getDailySongsDate();
  if (music.getDailySongs.length !== 0 && music.getDailySongsDate === dailySongsDate) {
    songsLoading.value = false;
    return;
  }

  const requestDate = selectedDate.value;
  songsLoading.value = true;
  try {
    const res = await getDailySongs();
    const dailySongs = extractDailySongs(res);
    if (Array.isArray(dailySongs)) {
      music.setDailySongs(dailySongs, dailySongsDate);
    } else {
      $message.error(t("general.message.acquisitionFailed"));
    }
  } catch (err) {
    console.error("Daily songs acquisition failed", err);
    $message.error(t("general.message.acquisitionFailed"));
  } finally {
    if (selectedDate.value === requestDate) songsLoading.value = false;
  }
};

// 获取历史日推可用日期
const getDailySongsHistoryDates = async () => {
  historyDatesLoading.value = true;
  try {
    const res = await getDailySongsHistory();
    historyDates.value = Array.isArray(res.data?.dates) ? res.data.dates : [];
  } catch (err) {
    console.error("Daily songs history dates acquisition failed", err);
  } finally {
    historyDatesLoading.value = false;
  }
};

// 获取历史日推详情
const getDailySongsHistoryData = async (date: string) => {
  if (historySongsCache.value[date]) {
    songsLoading.value = false;
    return;
  }
  const requestDate = date;
  songsLoading.value = true;
  try {
    const res = await getDailySongsHistoryDetail(date);
    const songs = extractDailySongs(res);
    if (Array.isArray(songs)) {
      historySongsCache.value[date] = transformSongData(songs);
    } else {
      historySongsCache.value[date] = [];
      $message.error(t("general.message.acquisitionFailed"));
    }
  } catch (err) {
    console.error("Daily songs history acquisition failed", err);
    $message.error(t("general.message.acquisitionFailed"));
  } finally {
    if (selectedDate.value === requestDate) songsLoading.value = false;
  }
};

const handleDateChange = (date: string) => {
  selectedDate.value = date;
  if (date === todayValue) {
    getDailySongsData();
  } else {
    getDailySongsHistoryData(date);
  }
};

onMounted(() => {
  $setSiteTitle(t("home.modules.dailySongs.title"));
  getDailySongsData();
  getDailySongsHistoryDates();
});
</script>

<style lang="scss" scoped>
.dailysongs {
  display: flex;
  flex-direction: column;
  padding: 6px 0 32px;

  .daily-head {
    margin-bottom: 24px;
    display: flex;
    align-items: center;
    gap: 16px;
    min-width: 0;

    .date-tile {
      --size: clamp(64px, 9vw, 96px);
      position: relative;
      width: var(--size);
      height: var(--size);
      flex-shrink: 0;
      display: grid;
      place-items: center;
      border-radius: var(--radius-md);
      color: var(--main-color);
      background: color-mix(in srgb, var(--main-color) 12%, transparent);

      .calendar {
        position: absolute;
        top: 10%;
        font-size: calc(var(--size) * 0.22);
        opacity: 0.7;
      }

      .day {
        margin-top: 12%;
        font-size: calc(var(--size) * 0.42);
        font-weight: 700;
        font-variant-numeric: tabular-nums;
        line-height: 1;
      }
    }

    .text {
      display: flex;
      flex-direction: column;
      flex: 1;
      min-width: 0;

      .eyebrow {
        margin-bottom: 10px;
        color: var(--n-text-color-3);
      }

      .title {
        margin: 0;
        font-size: clamp(26px, 4.6vw, 50px);
        font-weight: 700;
        line-height: 1.08;
        letter-spacing: -0.02em;
        color: var(--n-text-color);
      }

      .tip {
        margin-top: 8px;
        font-size: 14px;
      }
    }

    .history-select {
      width: 190px;
      flex-shrink: 0;
    }

    @media (max-width: 768px) {
      flex-wrap: wrap;

      .history-select {
        width: 100%;
      }
    }
  }

}
</style>
