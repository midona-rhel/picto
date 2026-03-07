/**
 * Typed command types — generated from Rust via ts-rs.
 *
 * Individual type files are auto-generated. This barrel + TypedCommandMap
 * is manually maintained but CI-validated against Rust definitions.
 *
 * Regenerate types: `cargo test --lib export_bindings`
 */

// Re-export generated input types
export type { ImportFilesInput } from './ImportFilesInput';
export type { UpdateFileStatusInput } from './UpdateFileStatusInput';
export type { DeleteFileInput } from './DeleteFileInput';
export type { DeleteFilesInput } from './DeleteFilesInput';

// Re-export generated output types
export type { ImportResult } from './ImportResult';
export type { ImportBatchResult } from './ImportBatchResult';

// Command name → { input, output } map for compile-time checked dispatch.
// Every typed command in Rust must have an entry here.
import type { ImportFilesInput } from './ImportFilesInput';
import type { UpdateFileStatusInput } from './UpdateFileStatusInput';
import type { DeleteFileInput } from './DeleteFileInput';
import type { DeleteFilesInput } from './DeleteFilesInput';
import type { ImportBatchResult } from './ImportBatchResult';

export interface TypedCommandMap {
  import_files: { input: ImportFilesInput; output: ImportBatchResult };
  update_file_status: { input: UpdateFileStatusInput; output: null };
  delete_file: { input: DeleteFileInput; output: null };
  delete_files: { input: DeleteFilesInput; output: number };
  rebuild_file_fts: { input: Record<string, never>; output: null };
  wipe_image_data: { input: Record<string, never>; output: null };
}
