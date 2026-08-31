
import { t } from '../../i18n';export type NotificationTone = 'error' | 'warning' | 'info' | 'success';

export interface NotificationPopupPreferences {
  enabled: boolean;
  tones: readonly NotificationTone[];
}

export interface ShowNotificationOptions {
  title: string;
  message: string;
  duration?: number;
  action?: {
    label: string;
    onClick: () => void;
  };
}

export interface AppNotification extends ShowNotificationOptions {
  id: number;
  tone: NotificationTone;
}

type Listener = () => void;

const listeners = new Set<Listener>();
let current: AppNotification | null = null;
let nextId = 1;
let popupPreferences: NotificationPopupPreferences = {
  enabled: true,
  tones: ['error', 'warning', 'info', 'success'],
};

function emit(): void {
  listeners.forEach((listener) => listener());
}

function showNotification(tone: NotificationTone, options: ShowNotificationOptions): void {
  current = {
    ...options,
    id: nextId++,
    tone,
    duration: options.duration ?? 4_000,
  };
  emit();
}

function isFolderDepthLimit(message: string): boolean {
  return message.includes('folders may be nested at most 8 levels deep');
}

export function subscribeToNotifications(listener: Listener): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

export function getCurrentNotification(): AppNotification | null {
  return current;
}

export function getNotificationPopupPreferences(): NotificationPopupPreferences {
  return popupPreferences;
}

export function configureNotificationPopups(next: NotificationPopupPreferences): void {
  popupPreferences = {
    enabled: next.enabled,
    tones: [...next.tones],
  };
}

export function dismissNotification(id: number): void {
  if (current?.id !== id) return;
  current = null;
  emit();
}

export function clearNotifications(): void {
  if (!current) return;
  current = null;
  emit();
}

export function showErrorNotification(options: ShowNotificationOptions): void {
  if (isFolderDepthLimit(options.message)) {
    showNotification('warning', {
      ...options,
      title: t("Folder depth limit"),
      message: 'Folders can be nested up to 8 levels. Choose a higher destination or flatten the folder structure.',
    });
    return;
  }
  showNotification('error', options);
}

export function showWarningNotification(options: ShowNotificationOptions): void {
  showNotification('warning', options);
}

export function showInfoNotification(options: ShowNotificationOptions): void {
  showNotification('info', options);
}

export function showSuccessNotification(options: ShowNotificationOptions): void {
  showNotification('success', options);
}
