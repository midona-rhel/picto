import { api } from '#desktop/api';
import type { AiTagPredictOutput, AiTaggerStatus } from '../shared/types/api';
import { canStartTaskFamily, isTaskFamilyRunning, useTaskStore, type TaskProgress } from '../state/taskStore';

export const aiTaggerController = {
  canStart(): { allowed: boolean; reason?: string } {
    return canStartTaskFamily('ai_tagger');
  },

  isRunning(): boolean {
    return isTaskFamilyRunning('ai_tagger');
  },

  /** Called when model download progress updates. */
  updateProgress(progress: TaskProgress) {
    const store = useTaskStore.getState();
    if (!store.familyProgress.ai_tagger.running) {
      store.startFamily('ai_tagger');
    }
    store.updateFamilyProgress('ai_tagger', progress);
  },

  finish() {
    useTaskStore.getState().finishFamily('ai_tagger');
  },

  status(): Promise<AiTaggerStatus> {
    return api.aiTagger.status();
  },

  downloadModel(model: string) {
    const check = canStartTaskFamily('ai_tagger');
    if (!check.allowed) return Promise.reject(new Error(check.reason ?? 'Model download blocked'));
    useTaskStore.getState().startFamily('ai_tagger');
    return api.aiTagger.downloadModel(model);
  },

  deleteModel(model: string) {
    return api.aiTagger.deleteModel(model);
  },

  async predict(hashes: string[], models?: string[]): Promise<AiTagPredictOutput> {
    useTaskStore.getState().startFamily('ai_tagger');
    try {
      const result = await api.aiTagger.predict(hashes, models);
      return result;
    } finally {
      useTaskStore.getState().finishFamily('ai_tagger');
    }
  },

  async apply(hashes: string[], tags: string[]) {
    useTaskStore.getState().startFamily('ai_tagger');
    try {
      await api.aiTagger.apply(hashes, tags);
    } finally {
      useTaskStore.getState().finishFamily('ai_tagger');
    }
  },
};
