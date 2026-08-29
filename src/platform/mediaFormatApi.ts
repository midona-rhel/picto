import { invoke } from './ipc';

export interface AcceptedMediaFormat {
  extension: string;
  mime_type: string;
}

let acceptedFormats: Promise<AcceptedMediaFormat[]> | null = null;

export function listAcceptedMediaFormats(): Promise<AcceptedMediaFormat[]> {
  acceptedFormats ??= invoke<AcceptedMediaFormat[]>('media.formats.list', {}).catch((error) => {
    acceptedFormats = null;
    throw error;
  });
  return acceptedFormats;
}
