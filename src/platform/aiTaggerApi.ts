import { invoke } from './ipc';
import type { AiAssignmentsInput } from '../shared/types/generated/application/AiAssignmentsInput';
import type { AiModelStatus } from '../shared/types/generated/application/AiModelStatus';
import type { AiRuntimeStatus } from '../shared/types/generated/application/AiRuntimeStatus';
import type { AiTagAssignment } from '../shared/types/generated/application/AiTagAssignment';
import type { AiTagPrediction } from '../shared/types/generated/application/AiTagPrediction';
import type { ManualPredictionResponse } from '../shared/types/generated/application/ManualPredictionResponse';
import type { MediaPrediction } from '../shared/types/generated/application/MediaPrediction';
import type { ModelInput } from '../shared/types/generated/application/ModelInput';
import type { MutationReceipt } from '../shared/types/generated/application/MutationReceipt';

export type {
  AiAssignmentsInput,
  AiModelStatus,
  AiRuntimeStatus,
  AiTagAssignment,
  AiTagPrediction,
  ManualPredictionResponse,
  MediaPrediction,
};

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

/** Predict tags for logical media items; physical file hashes stay backend-only. */
export function aiTagPredict(itemIds: number[], models?: string[]): Promise<ManualPredictionResponse> {
  return invoke<ManualPredictionResponse>('ai.review.predict', {
    itemIds,
    modelSlugs: models ?? null,
  });
}

/** Apply reviewed suggestions through the canonical item mutation path. */
export function aiTagApply(assignments: AiTagAssignment[]): Promise<MutationReceipt> {
  const input: AiAssignmentsInput = { assignments };
  return invoke<MutationReceipt>('ai.review.apply', input);
}
