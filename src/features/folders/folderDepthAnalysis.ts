import type { FolderTreeAnalysis } from '../../shared/types/generated/application/FolderTreeAnalysis';
import { t } from '../../i18n';

export function folderConsolidationMessage(analysis: FolderTreeAnalysis | null): string | null {
  if (!analysis || analysis.consolidated_levels === 0) return null;
  if (analysis.retained_depth === 0) {
    return t("This Picto folder is already at level 8. Files from the selected folder's subfolders will be added directly here, so no media is skipped because of folder depth.");
  }
  const levelLabel = analysis.retained_depth === 1 ? t('level') : t('levels');
  return t("This folder goes past Picto's 8-level folder limit. Picto can keep the first {value0} {value1} here. Files from deeper folders will be placed in the nearest kept folder, so no media is skipped because of folder depth.", {
    value0: analysis.retained_depth,
    value1: levelLabel,
  });
}
