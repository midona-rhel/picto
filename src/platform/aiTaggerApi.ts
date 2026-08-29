import { invoke } from './ipc';
import type { AiModelStatus } from '../shared/types/generated/application/AiModelStatus';
import type { AiRuntimeStatus } from '../shared/types/generated/application/AiRuntimeStatus';
import type { AiTagPrediction } from '../shared/types/generated/application/AiTagPrediction';
import type { ModelInput } from '../shared/types/generated/application/ModelInput';
import type { MutationReceipt } from '../shared/types/generated/application/MutationReceipt';

export type {
  AiModelStatus,
  AiRuntimeStatus,
  AiTagPrediction,
};

export interface RootPrediction {
  rootId: number;
  predictions: AiTagPrediction[];
  error?: string | null;
}

export interface LibraryManualPredictionResponse {
  predictions: RootPrediction[];
  thresholds: import('../shared/types/generated/application/AiThresholds').AiThresholds;
}

export interface RootTagAssignment {
  root_id: number;
  tags: string[];
}

/** Read the backend-owned AI model and threshold state. */
export function aiTaggerStatus(): Promise<AiRuntimeStatus> {
  return invoke<AiRuntimeStatus>('ai.status');
}

/** This completes when the model is downloaded or fails. */
export function aiTaggerDownloadModel(slug: string): Promise<AiRuntimeStatus> {
  const input: ModelInput = { slug };
  return invoke<AiRuntimeStatus>('ai.models.download', input);
}

export function aiTaggerCancelDownload(slug: string): Promise<void> {
  const input: ModelInput = { slug };
  return invoke<void>('ai.models.cancel', input);
}

export function aiTaggerDeleteModel(slug: string): Promise<AiRuntimeStatus> {
  const input: ModelInput = { slug };
  return invoke<AiRuntimeStatus>('ai.models.delete', input);
}

export function aiTaggerOptimizeModel(slug: string): Promise<AiRuntimeStatus> {
  const input: ModelInput = { slug };
  return invoke<AiRuntimeStatus>('ai.models.optimize', input);
}

/** Predict tags for logical media items; physical file hashes stay backend-only. */
export function aiTagPredict(rootIds: number[], models?: string[]): Promise<LibraryManualPredictionResponse> {
  return invoke<LibraryManualPredictionResponse>('ai.review.predict', {
    rootIds,
    modelSlugs: models ?? null,
  });
}

/** Stop manual review work and release its currently loaded model session. */
export function aiTaggerUnload(): Promise<void> {
  return invoke<void>('ai.review.unload');
}

/** Apply reviewed suggestions through the canonical item mutation path. */
export function aiTagApply(assignments: RootTagAssignment[]): Promise<MutationReceipt> {
  return invoke<MutationReceipt>('ai.review.apply', { assignments });
}
