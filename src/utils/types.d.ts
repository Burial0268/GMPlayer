declare module '@/utils/getLanguageData' {
  const getLanguageData: (key: string) => string;
  export default getLanguageData;
}

declare module '@/utils/timeTools' {
  export function formatNumber(num: number): string;
  export function getLongTime(time: number): string;
  export function getSongTime(time: number): string;
  export function getSongPlayingTime(time: number): string;
} 