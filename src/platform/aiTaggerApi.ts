import { invoke } from './ipc';

export interface AiTaggerStatus {
  models: Array<{
    model: string;
    downloaded: boolean;
    enabled: boolean;
    size_bytes: number | null;
  }>;
  gpu_backend: string | null;
}

export interface TagPrediction {
  namespace: string;
  tag: string;
  confidence: number;
}

export interface FilePrediction {
  hash: string;
  tags: TagPrediction[];
  error?: string | null;
}

export function aiTaggerStatus(): Promise<AiTaggerStatus> {
  return invoke<AiTaggerStatus>('ai_tagger_status');
}

export function aiTaggerDownloadModel(model: string): Promise<void> {
  return invoke<void>('ai_tagger_download_model', { model });
}

export function aiTaggerDeleteModel(model: string): Promise<void> {
  return invoke<void>('ai_tagger_delete_model', { model });
}

export function aiTagPredict(hashes: string[], models?: string[]): Promise<{ predictions: FilePrediction[] }> {
  return invoke<{ predictions: FilePrediction[] }>('ai_tag_predict', {
    hashes,
    models: models ?? null,
  } as unknown as Record<string, unknown>);
}

export function aiTagApply(hashes: string[], tags: string[]): Promise<{ applied_count: number }> {
  return invoke<{ applied_count: number }>('ai_tag_apply', { hashes, tags });
}
