import { notifications } from '@mantine/notifications';

interface ShowNotificationOptions {
  title: string;
  message: string;
}

export function showErrorNotification({ title, message }: ShowNotificationOptions): void {
  notifications.show({
    title,
    message,
    color: 'red',
    autoClose: 5_000,
  });
}

export function showWarningNotification({ title, message }: ShowNotificationOptions): void {
  notifications.show({
    title,
    message,
    color: 'yellow',
    autoClose: 5_000,
  });
}

export function showInfoNotification({ title, message }: ShowNotificationOptions): void {
  notifications.show({
    title,
    message,
    color: 'blue',
    autoClose: 5_000,
  });
}
