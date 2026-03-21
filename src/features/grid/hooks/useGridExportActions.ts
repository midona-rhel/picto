import { useCallback, useEffect, useMemo, useState } from 'react';

import { api, open } from '#desktop/api';
import { useExportActionStore } from '../../../state/exportActionStore';
import { useExportProgressStore } from '../../../state/exportProgressStore';
import { notifyError, notifySuccess, notifyWarning } from '../../../shared/lib/notify';
import { virtualSelectionSpec } from '../runtime/gridRuntimeSelectors';
import type { ExportMediaInput, SelectionQuerySpec as ExportSelectionQuerySpec } from '../../../shared/types/generated/commands';
import type { GridRuntimeState } from '../runtime';
import type { SelectionQuerySpec } from '../metadataPrefetch';

export interface ExportDialogState {
  outputDir: string;
  originalFormat: boolean;
  format: 'png' | 'jpg' | 'webp' | 'avif';
  quality: number;
  width: number | null;
  height: number | null;
  keepAspect: boolean;
}

interface ExportTarget {
  hashes: string[] | null;
  selection: ExportSelectionQuerySpec | null;
  total: number;
}

const DEFAULT_DIALOG_STATE: ExportDialogState = {
  outputDir: '',
  originalFormat: true,
  format: 'jpg',
  quality: 82,
  width: null,
  height: null,
  keepAspect: true,
};

function summarize(result: { exported: number; skipped: number; errors: number }): string {
  return `${result.exported} exported, ${result.skipped} skipped, ${result.errors} errors`;
}

function normalizeSelection(selection: SelectionQuerySpec): ExportMediaInput['selection'] {
  return selection as ExportSelectionQuerySpec;
}

export function useGridExportActions(args: {
  stateRef: React.MutableRefObject<GridRuntimeState>;
  selectedScopeCount: number | null;
}) {
  const { stateRef } = args;
  const [dialogOpen, setDialogOpen] = useState(false);
  const [dialogState, setDialogState] = useState<ExportDialogState>(DEFAULT_DIALOG_STATE);
  const [lastOutputDir, setLastOutputDir] = useState('');

  const exportRequestToken = useExportActionStore((s) => s.requestToken);
  const exportHandledToken = useExportActionStore((s) => s.handledToken);
  const exportRequestKind = useExportActionStore((s) => s.requestKind);
  const markExportHandled = useExportActionStore((s) => s.markHandled);

  const buildExportTarget = useCallback((): ExportTarget | null => {
    const virtualSpec = virtualSelectionSpec(stateRef.current);
    if (virtualSpec) {
      const total = stateRef.current.responseTotalCount;
      return total != null && total > 0
        ? { hashes: null, selection: normalizeSelection(virtualSpec), total }
        : null;
    }

    const visibleByHash = new Map(stateRef.current.images.map((image) => [image.hash, image]));
    const hashes = [...stateRef.current.selectedHashes].filter((hash) => {
      const image = visibleByHash.get(hash);
      return image?.is_collection !== true;
    });
    if (hashes.length === 0) return null;
    return { hashes, selection: null, total: hashes.length };
  }, [stateRef]);

  const selectOutputDir = useCallback(async (): Promise<string | null> => {
    const selected = await open({
      properties: ['openDirectory'],
      message: 'Choose export destination',
    });
    const picked = Array.isArray(selected) ? selected[0] : selected;
    if (!picked) return null;
    setLastOutputDir(picked);
    setDialogState((current) => ({ ...current, outputDir: picked }));
    return picked;
  }, []);

  const runExport = useCallback(async (
    target: ExportTarget,
    options: {
      outputDir: string;
      format: string | null;
      quality: number | null;
      width: number | null;
      height: number | null;
      keepAspect: boolean;
      label: string;
      successTitle: string;
    },
  ) => {
    const progress = useExportProgressStore.getState();
    progress.start(target.total, options.label);
    try {
      const result = await api.export.run({
        hashes: target.hashes,
        selection: target.selection,
        output_dir: options.outputDir,
        format: options.format,
        quality: options.quality,
        width: options.width,
        height: options.height,
        keep_aspect: options.keepAspect,
      });
      progress.finish(result);
      notifySuccess(summarize(result), options.successTitle);
    } catch (err) {
      progress.fail();
      notifyError(err, 'Export Failed');
    }
  }, []);

  const handleBasicExport = useCallback(async () => {
    const target = buildExportTarget();
    if (!target) {
      const title = stateRef.current.virtualAllSelection ? 'Count Unavailable' : 'Nothing Selected';
      const message = stateRef.current.virtualAllSelection
        ? 'Wait for the grid total to load before exporting all results.'
        : 'Select at least one file to export.';
      notifyWarning(message, title);
      return;
    }
    const outputDir = await selectOutputDir();
    if (!outputDir) return;
    await runExport(target, {
      outputDir,
      format: null,
      quality: null,
      width: null,
      height: null,
      keepAspect: true,
      label: 'Exporting originals',
      successTitle: 'Export Complete',
    });
  }, [buildExportTarget, runExport, selectOutputDir]);

  const openAdvancedExport = useCallback(() => {
    const target = buildExportTarget();
    if (!target) {
      const title = stateRef.current.virtualAllSelection ? 'Count Unavailable' : 'Nothing Selected';
      const message = stateRef.current.virtualAllSelection
        ? 'Wait for the grid total to load before exporting all results.'
        : 'Select at least one file to export.';
      notifyWarning(message, title);
      return;
    }
    setDialogState((current) => ({
      ...DEFAULT_DIALOG_STATE,
      outputDir: current.outputDir || lastOutputDir,
    }));
    setDialogOpen(true);
  }, [buildExportTarget, lastOutputDir]);

  const handleConfirmAdvancedExport = useCallback(async () => {
    const target = buildExportTarget();
    if (!target) {
      setDialogOpen(false);
      notifyWarning('Select at least one file to export.', 'Nothing Selected');
      return;
    }
    if (!dialogState.outputDir) {
      notifyWarning('Choose an export destination first.', 'Missing Destination');
      return;
    }
    setDialogOpen(false);
    await runExport(target, {
      outputDir: dialogState.outputDir,
      format: dialogState.originalFormat ? null : dialogState.format,
      quality: dialogState.originalFormat ? null : dialogState.quality,
      width: dialogState.originalFormat ? null : dialogState.width,
      height: dialogState.originalFormat ? null : dialogState.height,
      keepAspect: dialogState.originalFormat ? true : dialogState.keepAspect,
      label: dialogState.originalFormat ? 'Exporting originals' : `Exporting ${dialogState.format.toUpperCase()}`,
      successTitle: 'Export Complete',
    });
  }, [buildExportTarget, dialogState, runExport]);

  useEffect(() => {
    if (exportRequestToken === exportHandledToken) return;
    markExportHandled(exportRequestToken);
    if (exportRequestKind === 'advanced') {
      openAdvancedExport();
      return;
    }
    void handleBasicExport();
  }, [
    exportHandledToken,
    exportRequestKind,
    exportRequestToken,
    handleBasicExport,
    markExportHandled,
    openAdvancedExport,
  ]);

  const canConfirmAdvancedExport = useMemo(
    () => dialogState.outputDir.trim().length > 0,
    [dialogState.outputDir],
  );

  return {
    dialogOpen,
    setDialogOpen,
    dialogState,
    setDialogState,
    canConfirmAdvancedExport,
    selectOutputDir,
    handleBasicExport,
    openAdvancedExport,
    handleConfirmAdvancedExport,
  };
}
