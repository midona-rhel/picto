import { invoke } from './ipc';
import type { AiTaggerStatusOutput } from '../shared/types/generated/commands/AiTaggerStatusOutput';
import type { AiTagPredictOutput } from '../shared/types/generated/commands/AiTagPredictOutput';

export type { AiTaggerStatusOutput } from '../shared/types/generated/commands/AiTaggerStatusOutput';
export type { AiTaggerModelStatus } from '../shared/types/generated/commands/AiTaggerModelStatus';
export type { AiTaggerHardware } from '../shared/types/generated/commands/AiTaggerHardware';
export type { AiTagPredictOutput } from '../shared/types/generated/commands/AiTagPredictOutput';
export type { FilePrediction } from '../shared/types/generated/commands/FilePrediction';
export type { TagPrediction } from '../shared/types/generated/commands/TagPrediction';
export type { ModelInfo } from '../shared/types/generated/commands/ModelInfo';

export function aiTaggerStatus(): Promise<AiTaggerStatusOutput> {
  return invoke<AiTaggerStatusOutput>('ai_tagger_status', {});
}

export function aiTaggerDownloadModel(model: string): Promise<void> {
  return invoke<void>('ai_tagger_download_model', { model });
}

export function aiTaggerDeleteModel(model: string): Promise<void> {
  return invoke<void>('ai_tagger_delete_model', { model });
}

export function aiTagPredict(hashes: string[], models?: string[]): Promise<AiTagPredictOutput> {
  return invoke<AiTagPredictOutput>('ai_tag_predict', {
    hashes,
    models: models ?? null,
  });
}

export function aiTagCancel(): Promise<void> {
  return invoke<void>('ai_tag_cancel', {});
}

/** Apply tags to files. Returns the number of (file, tag) writes. */
export function aiTagApply(hashes: string[], tags: string[]): Promise<number> {
  return invoke<number>('ai_tag_apply', { hashes, tags });
}
