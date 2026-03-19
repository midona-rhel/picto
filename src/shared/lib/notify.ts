import { addToast } from '../components/ToastStack';

export function notifySuccess(message: string, _title?: string): void {
  addToast(message, 'success');
}

export function notifyError(message: string | unknown, _title?: string): void {
  addToast(String(message), 'error');
}

export function notifyWarning(message: string, _title?: string): void {
  addToast(message, 'warning');
}

export function notifyInfo(message: string, _title?: string): void {
  addToast(message, 'info');
}
