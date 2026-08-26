export type PlatformFamily = 'mac' | 'windows' | 'linux';

const MAC_UI_FONT = '"SF Pro Text", -apple-system, BlinkMacSystemFont, "Helvetica Neue", sans-serif';
const PORTABLE_UI_FONT = '"Geist", system-ui, "Segoe UI", "Helvetica Neue", "Noto Sans", "Liberation Sans", Arial, "PingFang SC", "PingFang TC", "Hiragino Sans GB", "Microsoft Yahei", sans-serif';

export function platformFamily(
  platform = navigator.platform,
  userAgent = navigator.userAgent,
): PlatformFamily {
  const normalized = (platform.trim() || userAgent).toLowerCase();
  if (normalized.includes('mac')) return 'mac';
  if (normalized.includes('win')) return 'windows';
  return 'linux';
}

export function publishPlatform(platform: PlatformFamily = platformFamily()): PlatformFamily {
  document.documentElement.dataset.platform = platform;
  return platform;
}

export function uiFontStack(platform: PlatformFamily = platformFamily()): string {
  return platform === 'mac' ? MAC_UI_FONT : PORTABLE_UI_FONT;
}
