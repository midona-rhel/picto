/**
 * Typed command types — generated from Rust via ts-rs.
 *
 * Individual type files are auto-generated. This barrel + TypedCommandMap
 * is manually maintained but CI-validated against Rust definitions.
 *
 * Regenerate types: `cargo test --lib export_bindings`
 */

// Re-export generated input types — files_lifecycle
export type { ImportFilesInput } from './ImportFilesInput';
export type { UpdateFileStatusInput } from './UpdateFileStatusInput';
export type { DeleteFileInput } from './DeleteFileInput';
export type { DeleteFilesInput } from './DeleteFilesInput';

// Re-export generated input types — folders
export type { GetFolderFilesInput } from './GetFolderFilesInput';
export type { GetFolderCoverHashInput } from './GetFolderCoverHashInput';
export type { GetFileFoldersInput } from './GetFileFoldersInput';
export type { GetEntityFoldersInput } from './GetEntityFoldersInput';
export type { MoveFolderInput } from './MoveFolderInput';
export type { CreateFolderInput } from './CreateFolderInput';
export type { UpdateFolderInput } from './UpdateFolderInput';
export type { DeleteFolderInput } from './DeleteFolderInput';
export type { UpdateFolderParentInput } from './UpdateFolderParentInput';
export type { AddFileToFolderInput } from './AddFileToFolderInput';
export type { AddFilesToFolderBatchInput } from './AddFilesToFolderBatchInput';
export type { RemoveFileFromFolderInput } from './RemoveFileFromFolderInput';
export type { RemoveFilesFromFolderBatchInput } from './RemoveFilesFromFolderBatchInput';
export type { ReorderFoldersInput } from './ReorderFoldersInput';
export type { ReorderFolderItemsInput } from './ReorderFolderItemsInput';
export type { SortFolderItemsInput } from './SortFolderItemsInput';
export type { ReverseFolderItemsInput } from './ReverseFolderItemsInput';
export type { GetCollectionSummaryInput } from './GetCollectionSummaryInput';
export type { CreateCollectionInput } from './CreateCollectionInput';
export type { UpdateCollectionInput } from './UpdateCollectionInput';
export type { SetCollectionRatingInput } from './SetCollectionRatingInput';
export type { SetCollectionSourceUrlsInput } from './SetCollectionSourceUrlsInput';
export type { ReorderCollectionMembersInput } from './ReorderCollectionMembersInput';
export type { AddCollectionMembersInput } from './AddCollectionMembersInput';
export type { RemoveCollectionMembersInput } from './RemoveCollectionMembersInput';
export type { DeleteCollectionInput } from './DeleteCollectionInput';

// Re-export generated input types — tags
export type { SearchTagsInput } from './SearchTagsInput';
export type { SearchTagsPagedInput } from './SearchTagsPagedInput';
export type { GetFileTagsInput } from './GetFileTagsInput';
export type { AddTagsInput } from './AddTagsInput';
export type { RemoveTagsInput } from './RemoveTagsInput';
export type { AddTagsBatchInput } from './AddTagsBatchInput';
export type { RemoveTagsBatchInput } from './RemoveTagsBatchInput';
export type { FindFilesByTagsInput } from './FindFilesByTagsInput';
export type { SetTagAliasInput } from './SetTagAliasInput';
export type { RemoveTagAliasInput } from './RemoveTagAliasInput';
export type { GetTagSiblingsInput } from './GetTagSiblingsInput';
export type { GetTagParentsInput } from './GetTagParentsInput';
export type { AddTagParentInput } from './AddTagParentInput';
export type { RemoveTagParentInput } from './RemoveTagParentInput';
export type { MergeTagsInput } from './MergeTagsInput';
export type { GetTagsPaginatedInput } from './GetTagsPaginatedInput';
export type { RenameTagInput } from './RenameTagInput';
export type { DeleteTagInput } from './DeleteTagInput';
export type { CompanionGetNamespaceValuesInput } from './CompanionGetNamespaceValuesInput';
export type { CompanionGetFilesByTagInput } from './CompanionGetFilesByTagInput';

// Re-export generated input types — selection
export type { AddTagsSelectionInput } from './AddTagsSelectionInput';
export type { RemoveTagsSelectionInput } from './RemoveTagsSelectionInput';
export type { GetSelectionSummaryInput } from './GetSelectionSummaryInput';
export type { UpdateRatingSelectionInput } from './UpdateRatingSelectionInput';
export type { SetNotesSelectionInput } from './SetNotesSelectionInput';
export type { SetSourceUrlsSelectionInput } from './SetSourceUrlsSelectionInput';

// Re-export generated output/shared types
export type { ImportResult } from './ImportResult';
export type { ImportBatchResult } from './ImportBatchResult';
export type { FolderReorderMove } from './FolderReorderMove';
export type { SelectionQuerySpec } from './SelectionQuerySpec';
export type { SelectionMode } from './SelectionMode';
export type { SmartFolderPredicate } from './SmartFolderPredicate';
export type { SmartRuleGroup } from './SmartRuleGroup';
export type { MatchMode } from './MatchMode';
export type { PredicateRule } from './PredicateRule';

// Command name → { input, output } map for compile-time checked dispatch.
// Every typed command in Rust must have an entry here.
import type { ImportFilesInput } from './ImportFilesInput';
import type { UpdateFileStatusInput } from './UpdateFileStatusInput';
import type { DeleteFileInput } from './DeleteFileInput';
import type { DeleteFilesInput } from './DeleteFilesInput';
import type { ImportBatchResult } from './ImportBatchResult';
import type { GetFolderFilesInput } from './GetFolderFilesInput';
import type { GetFolderCoverHashInput } from './GetFolderCoverHashInput';
import type { GetFileFoldersInput } from './GetFileFoldersInput';
import type { GetEntityFoldersInput } from './GetEntityFoldersInput';
import type { MoveFolderInput } from './MoveFolderInput';
import type { CreateFolderInput } from './CreateFolderInput';
import type { UpdateFolderInput } from './UpdateFolderInput';
import type { DeleteFolderInput } from './DeleteFolderInput';
import type { UpdateFolderParentInput } from './UpdateFolderParentInput';
import type { AddFileToFolderInput } from './AddFileToFolderInput';
import type { AddFilesToFolderBatchInput } from './AddFilesToFolderBatchInput';
import type { RemoveFileFromFolderInput } from './RemoveFileFromFolderInput';
import type { RemoveFilesFromFolderBatchInput } from './RemoveFilesFromFolderBatchInput';
import type { ReorderFoldersInput } from './ReorderFoldersInput';
import type { ReorderFolderItemsInput } from './ReorderFolderItemsInput';
import type { SortFolderItemsInput } from './SortFolderItemsInput';
import type { ReverseFolderItemsInput } from './ReverseFolderItemsInput';
import type { GetCollectionSummaryInput } from './GetCollectionSummaryInput';
import type { CreateCollectionInput } from './CreateCollectionInput';
import type { UpdateCollectionInput } from './UpdateCollectionInput';
import type { SetCollectionRatingInput } from './SetCollectionRatingInput';
import type { SetCollectionSourceUrlsInput } from './SetCollectionSourceUrlsInput';
import type { ReorderCollectionMembersInput } from './ReorderCollectionMembersInput';
import type { AddCollectionMembersInput } from './AddCollectionMembersInput';
import type { RemoveCollectionMembersInput } from './RemoveCollectionMembersInput';
import type { DeleteCollectionInput } from './DeleteCollectionInput';
import type { SearchTagsInput } from './SearchTagsInput';
import type { SearchTagsPagedInput } from './SearchTagsPagedInput';
import type { GetFileTagsInput } from './GetFileTagsInput';
import type { AddTagsInput } from './AddTagsInput';
import type { RemoveTagsInput } from './RemoveTagsInput';
import type { AddTagsBatchInput } from './AddTagsBatchInput';
import type { RemoveTagsBatchInput } from './RemoveTagsBatchInput';
import type { FindFilesByTagsInput } from './FindFilesByTagsInput';
import type { SetTagAliasInput } from './SetTagAliasInput';
import type { RemoveTagAliasInput } from './RemoveTagAliasInput';
import type { GetTagSiblingsInput } from './GetTagSiblingsInput';
import type { GetTagParentsInput } from './GetTagParentsInput';
import type { AddTagParentInput } from './AddTagParentInput';
import type { RemoveTagParentInput } from './RemoveTagParentInput';
import type { MergeTagsInput } from './MergeTagsInput';
import type { GetTagsPaginatedInput } from './GetTagsPaginatedInput';
import type { RenameTagInput } from './RenameTagInput';
import type { DeleteTagInput } from './DeleteTagInput';
import type { CompanionGetNamespaceValuesInput } from './CompanionGetNamespaceValuesInput';
import type { CompanionGetFilesByTagInput } from './CompanionGetFilesByTagInput';
import type { AddTagsSelectionInput } from './AddTagsSelectionInput';
import type { RemoveTagsSelectionInput } from './RemoveTagsSelectionInput';
import type { GetSelectionSummaryInput } from './GetSelectionSummaryInput';
import type { UpdateRatingSelectionInput } from './UpdateRatingSelectionInput';
import type { SetNotesSelectionInput } from './SetNotesSelectionInput';
import type { SetSourceUrlsSelectionInput } from './SetSourceUrlsSelectionInput';

export interface TypedCommandMap {
  // files_lifecycle
  import_files: { input: ImportFilesInput; output: ImportBatchResult };
  update_file_status: { input: UpdateFileStatusInput; output: null };
  delete_file: { input: DeleteFileInput; output: null };
  delete_files: { input: DeleteFilesInput; output: number };
  rebuild_file_fts: { input: Record<string, never>; output: null };
  wipe_image_data: { input: Record<string, never>; output: null };
  // folders
  list_folders: { input: Record<string, never>; output: unknown };
  get_folder_files: { input: GetFolderFilesInput; output: string[] };
  get_folder_cover_hash: { input: GetFolderCoverHashInput; output: string | null };
  get_file_folders: { input: GetFileFoldersInput; output: unknown };
  get_entity_folders: { input: GetEntityFoldersInput; output: unknown };
  move_folder: { input: MoveFolderInput; output: null };
  create_folder: { input: CreateFolderInput; output: unknown };
  update_folder: { input: UpdateFolderInput; output: null };
  delete_folder: { input: DeleteFolderInput; output: null };
  update_folder_parent: { input: UpdateFolderParentInput; output: null };
  add_file_to_folder: { input: AddFileToFolderInput; output: null };
  add_files_to_folder_batch: { input: AddFilesToFolderBatchInput; output: number };
  remove_file_from_folder: { input: RemoveFileFromFolderInput; output: null };
  remove_files_from_folder_batch: { input: RemoveFilesFromFolderBatchInput; output: number };
  reorder_folders: { input: ReorderFoldersInput; output: null };
  reorder_folder_items: { input: ReorderFolderItemsInput; output: null };
  sort_folder_items: { input: SortFolderItemsInput; output: null };
  reverse_folder_items: { input: ReverseFolderItemsInput; output: null };
  get_collections: { input: Record<string, never>; output: unknown };
  get_collection_summary: { input: GetCollectionSummaryInput; output: unknown };
  create_collection: { input: CreateCollectionInput; output: number };
  update_collection: { input: UpdateCollectionInput; output: null };
  set_collection_rating: { input: SetCollectionRatingInput; output: null };
  set_collection_source_urls: { input: SetCollectionSourceUrlsInput; output: null };
  reorder_collection_members: { input: ReorderCollectionMembersInput; output: null };
  add_collection_members: { input: AddCollectionMembersInput; output: number };
  remove_collection_members: { input: RemoveCollectionMembersInput; output: number };
  delete_collection: { input: DeleteCollectionInput; output: null };
  // tags
  search_tags: { input: SearchTagsInput; output: unknown };
  search_tags_paged: { input: SearchTagsPagedInput; output: unknown };
  get_all_tags_with_counts: { input: Record<string, never>; output: unknown };
  get_file_tags: { input: GetFileTagsInput; output: unknown };
  add_tags: { input: AddTagsInput; output: string[] };
  remove_tags: { input: RemoveTagsInput; output: null };
  add_tags_batch: { input: AddTagsBatchInput; output: null };
  remove_tags_batch: { input: RemoveTagsBatchInput; output: null };
  find_files_by_tags: { input: FindFilesByTagsInput; output: unknown };
  set_tag_alias: { input: SetTagAliasInput; output: null };
  remove_tag_alias: { input: RemoveTagAliasInput; output: null };
  get_tag_aliases: { input: Record<string, never>; output: unknown };
  get_tag_siblings_for_tag: { input: GetTagSiblingsInput; output: unknown };
  get_tag_parents_for_tag: { input: GetTagParentsInput; output: unknown };
  add_tag_parent: { input: AddTagParentInput; output: null };
  remove_tag_parent: { input: RemoveTagParentInput; output: null };
  merge_tags: { input: MergeTagsInput; output: null };
  lookup_tag_types: { input: Record<string, never>; output: string[] };
  get_tags_paginated: { input: GetTagsPaginatedInput; output: unknown };
  get_namespace_summary: { input: Record<string, never>; output: unknown };
  rename_tag: { input: RenameTagInput; output: unknown };
  delete_tag: { input: DeleteTagInput; output: unknown };
  normalize_ingested_namespaces: { input: Record<string, never>; output: unknown };
  companion_get_namespace_values: { input: CompanionGetNamespaceValuesInput; output: unknown };
  companion_get_files_by_tag: { input: CompanionGetFilesByTagInput; output: unknown };
  // selection
  add_tags_selection: { input: AddTagsSelectionInput; output: number };
  remove_tags_selection: { input: RemoveTagsSelectionInput; output: number };
  get_selection_summary: { input: GetSelectionSummaryInput; output: unknown };
  update_rating_selection: { input: UpdateRatingSelectionInput; output: number };
  set_notes_selection: { input: SetNotesSelectionInput; output: number };
  set_source_urls_selection: { input: SetSourceUrlsSelectionInput; output: number };
}
