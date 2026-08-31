import { invoke } from './ipc';
import type { EntityTarget } from '../shared/types/canonical';

export type PictoPackSource =
  | { kind: 'items'; target: EntityTarget }
  | { kind: 'folder'; folder_id: number }
  | { kind: 'smart_folder'; smart_folder_id: number };

export interface PictoPackSummary {
  name: string;
  source_kind: string;
  root_count: number;
  media_count: number;
  folder_count: number;
  smart_folder_count: number;
  total_bytes: number;
}

export interface PictoPackExportResult {
  output_path: string;
  summary: PictoPackSummary;
}

export interface PictoPackImportResult {
  imported_roots: number;
  imported_media: number;
  imported_folders: number;
  imported_smart_folders: number;
}

export function inspectPictoPack(path: string): Promise<PictoPackSummary> {
  return invoke('picto_pack.inspect', { path });
}

export function exportPictoPack(source: PictoPackSource, outputPath: string): Promise<PictoPackExportResult> {
  return invoke('picto_pack.export', { source, output_path: outputPath });
}

export function importPictoPack(path: string): Promise<PictoPackImportResult> {
  return invoke('picto_pack.import', { path });
}
