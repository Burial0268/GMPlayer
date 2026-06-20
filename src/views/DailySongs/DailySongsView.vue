<template>
  <div class="dailysongs">
    <div class="top">
      <div class="title">
        <div class="date">
          <n-icon class="calendar" :component="CalendarTodayFilled" />
          <n-text class="num">{{ displayDay }}</n-text>
        </div>
        <div class="right">
          <n-gradient-text class="big" type="danger">
            {{ $t("home.modules.dailySongs.title") }}
          </n-gradient-text>
          <n-text class="tip" :depth="3">
            {{ displaySubtitle }}
          </n-text>
        </div>
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
    </div>
    <DataLists :listData="displaySongs" :loading="songsLoading" />
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
  .top {
    margin: 30px 0 40px;
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 20px;
    flex-wrap: wrap;
    .title {
      font-size: 40px;
      display: flex;
      align-items: center;
      .calendar {
        font-size: 70px;
        color: #d03050;
        transform: translateY(-3px);
      }
      .date {
        position: relative;
        margin-right: 16px;
        display: flex;
        align-items: center;
        justify-content: center;
        .num {
          margin-top: 7px;
          position: absolute;
          font-size: 30px;
          font-weight: bold;
          color: #d03050;
        }
      }
      .right {
        display: flex;
        flex-direction: column;
        .big {
          --n-font-weight: bold;
          line-height: 50px;
        }
        .tip {
          font-size: 14px;
          margin-left: 2px;
        }
      }
    }
    .history-select {
      width: 190px;
    }
  }
  @media (max-width: 768px) {
    .top {
      align-items: flex-start;
      .history-select {
        width: 100%;
      }
    }
  }
}
</style>
