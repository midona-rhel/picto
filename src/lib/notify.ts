import { notifications } from '@mantine/notifications';

export function notifySuccess(message: string, title = 'Success'): void {
  notifications.show({ title, message, color: 'green', autoClose: 4000 });
}

export function notifyError(message: string | unknown, title = 'Error'): void {
  notifications.show({ title, message: String(message), color: 'red', autoClose: 6000 });
}

export function notifyWarning(message: string, title = 'Warning'): void {
  notifications.show({ title, message, color: 'yellow', autoClose: 5000 });
}

export function notifyInfo(message: string, title = 'Info'): void {
  notifications.show({ title, message, color: 'blue', autoClose: 4000 });
}
