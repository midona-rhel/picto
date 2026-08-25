export function formatRelativeTime(value: string | null | undefined): string {
  if (!value) return 'Never';
  const parsed = Date.parse(value);
  if (Number.isNaN(parsed)) return value;
  const deltaMinutes = Math.round((parsed - Date.now()) / 60000);
  const formatter = new Intl.RelativeTimeFormat(undefined, { numeric: 'auto' });
  if (Math.abs(deltaMinutes) < 60) return formatter.format(deltaMinutes, 'minute');
  const deltaHours = Math.round(deltaMinutes / 60);
  if (Math.abs(deltaHours) < 48) return formatter.format(deltaHours, 'hour');
  const deltaDays = Math.round(deltaHours / 24);
  return formatter.format(deltaDays, 'day');
}
