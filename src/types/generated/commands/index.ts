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
export type { DeleteFilesSelectionInput } from './DeleteFilesSelectionInput';
export type { UpdateFileStatusSelectionInput } from './UpdateFileStatusSelectionInput';

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

// Re-export generated input types — grid
export type { GetGridPageSlimInput } from './GetGridPageSlimInput';
export type { GetFileInput } from './GetFileInput';
export type { GetFilesMetadataBatchInput } from './GetFilesMetadataBatchInput';

// Re-export generated input types — files_metadata
export type { GetFileAllMetadataInput } from './GetFileAllMetadataInput';
export type { GetFileTagsDisplayInput } from './GetFileTagsDisplayInput';
export type { GetFileParentsInput } from './GetFileParentsInput';
export type { UpdateRatingInput } from './UpdateRatingInput';
export type { SetFileNameInput } from './SetFileNameInput';
export type { GetFileNotesInput } from './GetFileNotesInput';
export type { SetFileNotesInput } from './SetFileNotesInput';
export type { IncrementViewCountInput } from './IncrementViewCountInput';
export type { SetSourceUrlsInput } from './SetSourceUrlsInput';

// Re-export generated input types — files_media
export type { ResolveFilePathInput } from './ResolveFilePathInput';
export type { OpenFileDefaultInput } from './OpenFileDefaultInput';
export type { RevealInFolderInput } from './RevealInFolderInput';
export type { ExportFileInput } from './ExportFileInput';
export type { OpenInNewWindowInput } from './OpenInNewWindowInput';
export type { ResolveThumbnailPathInput } from './ResolveThumbnailPathInput';
export type { EnsureThumbnailInput } from './EnsureThumbnailInput';
export type { RegenerateThumbnailInput } from './RegenerateThumbnailInput';
export type { RegenerateThumbnailsBatchInput } from './RegenerateThumbnailsBatchInput';
export type { ReanalyzeFileColorsInput } from './ReanalyzeFileColorsInput';
export type { BackfillMissingBlurhashesInput } from './BackfillMissingBlurhashesInput';
export type { SearchByColorInput } from './SearchByColorInput';
export type { GetImageThumbnailInput } from './GetImageThumbnailInput';

// Re-export generated input types — subscriptions
export type { CreateFlowInput } from './CreateFlowInput';
export type { DeleteFlowInput } from './DeleteFlowInput';
export type { RenameFlowInput } from './RenameFlowInput';
export type { SetFlowScheduleInput } from './SetFlowScheduleInput';
export type { RunFlowInput } from './RunFlowInput';
export type { StopFlowInput } from './StopFlowInput';
export type { GetSiteMetadataSchemaInput } from './GetSiteMetadataSchemaInput';
export type { ValidateSiteMetadataInput } from './ValidateSiteMetadataInput';
export type { CreateSubscriptionInput } from './CreateSubscriptionInput';
export type { DeleteSubscriptionInput } from './DeleteSubscriptionInput';
export type { PauseSubscriptionInput } from './PauseSubscriptionInput';
export type { AddSubscriptionQueryInput } from './AddSubscriptionQueryInput';
export type { DeleteSubscriptionQueryInput } from './DeleteSubscriptionQueryInput';
export type { PauseSubscriptionQueryInput } from './PauseSubscriptionQueryInput';
export type { RunSubscriptionInput } from './RunSubscriptionInput';
export type { StopSubscriptionInput } from './StopSubscriptionInput';
export type { ResetSubscriptionInput } from './ResetSubscriptionInput';
export type { RenameSubscriptionInput } from './RenameSubscriptionInput';
export type { RunSubscriptionQueryInput } from './RunSubscriptionQueryInput';
export type { SetCredentialInput } from './SetCredentialInput';
export type { DeleteCredentialInput } from './DeleteCredentialInput';

// Re-export generated input types — ptr
export type { PtrGetTagsPaginatedInput } from './PtrGetTagsPaginatedInput';
export type { PtrGetTagRelationInput } from './PtrGetTagRelationInput';

// Re-export generated input types — system
export type { OpenExternalUrlInput } from './OpenExternalUrlInput';
export type { ReorderSidebarNodesInput } from './ReorderSidebarNodesInput';
export type { GetViewPrefsInput } from './GetViewPrefsInput';
export type { SetViewPrefsInput } from './SetViewPrefsInput';
export type { SetZoomFactorInput } from './SetZoomFactorInput';

// Re-export generated input types — duplicates
export type { GetDuplicatesInput } from './GetDuplicatesInput';
export type { ScanDuplicatesInput } from './ScanDuplicatesInput';
export type { GetDuplicatePairsInput } from './GetDuplicatePairsInput';
export type { ResolveDuplicatePairInput } from './ResolveDuplicatePairInput';
export type { UpdateDuplicateSettingsInput } from './UpdateDuplicateSettingsInput';

// Re-export generated input types — smart_folders
export type { ReorderSmartFoldersInput } from './ReorderSmartFoldersInput';
export type { CreateSmartFolderInput } from './CreateSmartFolderInput';
export type { UpdateSmartFolderInput } from './UpdateSmartFolderInput';
export type { DeleteSmartFolderInput } from './DeleteSmartFolderInput';
export type { QuerySmartFolderInput } from './QuerySmartFolderInput';
export type { CountSmartFolderInput } from './CountSmartFolderInput';

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
export type { GridPageSlimQuery } from './GridPageSlimQuery';
export type { ViewPrefsPatch } from './ViewPrefsPatch';

// Command name → { input, output } map for compile-time checked dispatch.
// Every typed command in Rust must have an entry here.
import type { ImportFilesInput } from './ImportFilesInput';
import type { UpdateFileStatusInput } from './UpdateFileStatusInput';
import type { DeleteFileInput } from './DeleteFileInput';
import type { DeleteFilesInput } from './DeleteFilesInput';
import type { DeleteFilesSelectionInput } from './DeleteFilesSelectionInput';
import type { UpdateFileStatusSelectionInput } from './UpdateFileStatusSelectionInput';
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
import type { GetGridPageSlimInput } from './GetGridPageSlimInput';
import type { GetFileInput } from './GetFileInput';
import type { GetFilesMetadataBatchInput } from './GetFilesMetadataBatchInput';
import type { GetFileAllMetadataInput } from './GetFileAllMetadataInput';
import type { GetFileTagsDisplayInput } from './GetFileTagsDisplayInput';
import type { GetFileParentsInput } from './GetFileParentsInput';
import type { UpdateRatingInput } from './UpdateRatingInput';
import type { SetFileNameInput } from './SetFileNameInput';
import type { GetFileNotesInput } from './GetFileNotesInput';
import type { SetFileNotesInput } from './SetFileNotesInput';
import type { IncrementViewCountInput } from './IncrementViewCountInput';
import type { SetSourceUrlsInput } from './SetSourceUrlsInput';
import type { ResolveFilePathInput } from './ResolveFilePathInput';
import type { OpenFileDefaultInput } from './OpenFileDefaultInput';
import type { RevealInFolderInput } from './RevealInFolderInput';
import type { ExportFileInput } from './ExportFileInput';
import type { OpenInNewWindowInput } from './OpenInNewWindowInput';
import type { ResolveThumbnailPathInput } from './ResolveThumbnailPathInput';
import type { EnsureThumbnailInput } from './EnsureThumbnailInput';
import type { RegenerateThumbnailInput } from './RegenerateThumbnailInput';
import type { RegenerateThumbnailsBatchInput } from './RegenerateThumbnailsBatchInput';
import type { ReanalyzeFileColorsInput } from './ReanalyzeFileColorsInput';
import type { BackfillMissingBlurhashesInput } from './BackfillMissingBlurhashesInput';
import type { SearchByColorInput } from './SearchByColorInput';
import type { GetImageThumbnailInput } from './GetImageThumbnailInput';
import type { CreateFlowInput } from './CreateFlowInput';
import type { DeleteFlowInput } from './DeleteFlowInput';
import type { RenameFlowInput } from './RenameFlowInput';
import type { SetFlowScheduleInput } from './SetFlowScheduleInput';
import type { RunFlowInput } from './RunFlowInput';
import type { StopFlowInput } from './StopFlowInput';
import type { GetSiteMetadataSchemaInput } from './GetSiteMetadataSchemaInput';
import type { ValidateSiteMetadataInput } from './ValidateSiteMetadataInput';
import type { CreateSubscriptionInput } from './CreateSubscriptionInput';
import type { DeleteSubscriptionInput } from './DeleteSubscriptionInput';
import type { PauseSubscriptionInput } from './PauseSubscriptionInput';
import type { AddSubscriptionQueryInput } from './AddSubscriptionQueryInput';
import type { DeleteSubscriptionQueryInput } from './DeleteSubscriptionQueryInput';
import type { PauseSubscriptionQueryInput } from './PauseSubscriptionQueryInput';
import type { RunSubscriptionInput } from './RunSubscriptionInput';
import type { StopSubscriptionInput } from './StopSubscriptionInput';
import type { ResetSubscriptionInput } from './ResetSubscriptionInput';
import type { RenameSubscriptionInput } from './RenameSubscriptionInput';
import type { RunSubscriptionQueryInput } from './RunSubscriptionQueryInput';
import type { SetCredentialInput } from './SetCredentialInput';
import type { DeleteCredentialInput } from './DeleteCredentialInput';
import type { PtrGetTagsPaginatedInput } from './PtrGetTagsPaginatedInput';
import type { PtrGetTagRelationInput } from './PtrGetTagRelationInput';
import type { OpenExternalUrlInput } from './OpenExternalUrlInput';
import type { ReorderSidebarNodesInput } from './ReorderSidebarNodesInput';
import type { GetViewPrefsInput } from './GetViewPrefsInput';
import type { SetViewPrefsInput } from './SetViewPrefsInput';
import type { SetZoomFactorInput } from './SetZoomFactorInput';
import type { GetDuplicatesInput } from './GetDuplicatesInput';
import type { ScanDuplicatesInput } from './ScanDuplicatesInput';
import type { GetDuplicatePairsInput } from './GetDuplicatePairsInput';
import type { ResolveDuplicatePairInput } from './ResolveDuplicatePairInput';
import type { UpdateDuplicateSettingsInput } from './UpdateDuplicateSettingsInput';
import type { ReorderSmartFoldersInput } from './ReorderSmartFoldersInput';
import type { CreateSmartFolderInput } from './CreateSmartFolderInput';
import type { UpdateSmartFolderInput } from './UpdateSmartFolderInput';
import type { DeleteSmartFolderInput } from './DeleteSmartFolderInput';
import type { QuerySmartFolderInput } from './QuerySmartFolderInput';
import type { CountSmartFolderInput } from './CountSmartFolderInput';

export interface TypedCommandMap {
  // files_lifecycle
  import_files: { input: ImportFilesInput; output: ImportBatchResult };
  update_file_status: { input: UpdateFileStatusInput; output: null };
  delete_file: { input: DeleteFileInput; output: null };
  delete_files: { input: DeleteFilesInput; output: number };
  rebuild_file_fts: { input: Record<string, never>; output: null };
  wipe_image_data: { input: Record<string, never>; output: null };
  delete_files_selection: { input: DeleteFilesSelectionInput; output: number };
  update_file_status_selection: { input: UpdateFileStatusSelectionInput; output: number };
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
  // grid
  get_grid_page_slim: { input: GetGridPageSlimInput; output: unknown };
  get_file: { input: GetFileInput; output: unknown };
  get_files_metadata_batch: { input: GetFilesMetadataBatchInput; output: unknown };
  get_file_count: { input: Record<string, never>; output: unknown };
  // files_metadata
  get_file_all_metadata: { input: GetFileAllMetadataInput; output: unknown };
  get_file_tags_display: { input: GetFileTagsDisplayInput; output: unknown };
  get_file_parents: { input: GetFileParentsInput; output: unknown };
  update_rating: { input: UpdateRatingInput; output: null };
  set_file_name: { input: SetFileNameInput; output: null };
  get_file_notes: { input: GetFileNotesInput; output: unknown };
  set_file_notes: { input: SetFileNotesInput; output: null };
  increment_view_count: { input: IncrementViewCountInput; output: null };
  set_source_urls: { input: SetSourceUrlsInput; output: null };
  get_storage_stats: { input: Record<string, never>; output: unknown };
  get_image_storage_stats: { input: Record<string, never>; output: unknown };
  // files_media
  resolve_file_path: { input: ResolveFilePathInput; output: string };
  open_file_default: { input: OpenFileDefaultInput; output: null };
  reveal_in_folder: { input: RevealInFolderInput; output: null };
  export_file: { input: ExportFileInput; output: null };
  open_in_new_window: { input: OpenInNewWindowInput; output: null };
  resolve_thumbnail_path: { input: ResolveThumbnailPathInput; output: string };
  ensure_thumbnail: { input: EnsureThumbnailInput; output: unknown };
  regenerate_thumbnail: { input: RegenerateThumbnailInput; output: unknown };
  regenerate_thumbnails_batch: { input: RegenerateThumbnailsBatchInput; output: unknown };
  reanalyze_file_colors: { input: ReanalyzeFileColorsInput; output: unknown };
  backfill_missing_blurhashes: { input: BackfillMissingBlurhashesInput; output: unknown };
  search_by_color: { input: SearchByColorInput; output: unknown };
  get_image_thumbnail: { input: GetImageThumbnailInput; output: unknown };
  // subscriptions
  get_flows: { input: Record<string, never>; output: unknown };
  create_flow: { input: CreateFlowInput; output: unknown };
  delete_flow: { input: DeleteFlowInput; output: null };
  rename_flow: { input: RenameFlowInput; output: null };
  set_flow_schedule: { input: SetFlowScheduleInput; output: null };
  run_flow: { input: RunFlowInput; output: null };
  stop_flow: { input: StopFlowInput; output: null };
  get_sites: { input: Record<string, never>; output: unknown };
  get_site_metadata_schema: { input: GetSiteMetadataSchemaInput; output: unknown };
  validate_site_metadata: { input: ValidateSiteMetadataInput; output: unknown };
  get_subscriptions: { input: Record<string, never>; output: unknown };
  create_subscription: { input: CreateSubscriptionInput; output: unknown };
  delete_subscription: { input: DeleteSubscriptionInput; output: unknown };
  pause_subscription: { input: PauseSubscriptionInput; output: null };
  add_subscription_query: { input: AddSubscriptionQueryInput; output: unknown };
  delete_subscription_query: { input: DeleteSubscriptionQueryInput; output: null };
  pause_subscription_query: { input: PauseSubscriptionQueryInput; output: null };
  run_subscription: { input: RunSubscriptionInput; output: null };
  stop_subscription: { input: StopSubscriptionInput; output: null };
  reset_subscription: { input: ResetSubscriptionInput; output: null };
  get_running_subscriptions: { input: Record<string, never>; output: unknown };
  get_running_subscription_progress: { input: Record<string, never>; output: unknown };
  rename_subscription: { input: RenameSubscriptionInput; output: null };
  run_subscription_query: { input: RunSubscriptionQueryInput; output: null };
  list_credentials: { input: Record<string, never>; output: unknown };
  list_credential_health: { input: Record<string, never>; output: unknown };
  set_credential: { input: SetCredentialInput; output: null };
  delete_credential: { input: DeleteCredentialInput; output: null };
  // ptr
  get_ptr_status: { input: Record<string, never>; output: unknown };
  is_ptr_syncing: { input: Record<string, never>; output: unknown };
  get_ptr_sync_progress: { input: Record<string, never>; output: unknown };
  ptr_sync: { input: Record<string, never>; output: unknown };
  cancel_ptr_sync: { input: Record<string, never>; output: null };
  ptr_cancel_bootstrap: { input: Record<string, never>; output: null };
  ptr_bootstrap_from_hydrus_snapshot: { input: Record<string, unknown>; output: unknown };
  ptr_get_bootstrap_status: { input: Record<string, never>; output: unknown };
  ptr_get_compact_index_status: { input: Record<string, never>; output: unknown };
  get_ptr_sync_perf_breakdown: { input: Record<string, never>; output: unknown };
  ptr_get_namespace_summary: { input: Record<string, never>; output: unknown };
  ptr_get_tags_paginated: { input: PtrGetTagsPaginatedInput; output: unknown };
  ptr_get_tag_siblings: { input: PtrGetTagRelationInput; output: unknown };
  ptr_get_tag_parents: { input: PtrGetTagRelationInput; output: unknown };
  // system
  get_settings: { input: Record<string, never>; output: unknown };
  save_settings: { input: Record<string, unknown>; output: null };
  get_library_info: { input: Record<string, never>; output: unknown };
  get_perf_snapshot: { input: Record<string, never>; output: unknown };
  check_perf_slo: { input: Record<string, never>; output: unknown };
  open_external_url: { input: OpenExternalUrlInput; output: null };
  get_sidebar_tree: { input: Record<string, never>; output: unknown };
  reorder_sidebar_nodes: { input: ReorderSidebarNodesInput; output: null };
  get_view_prefs: { input: GetViewPrefsInput; output: unknown };
  set_view_prefs: { input: SetViewPrefsInput; output: unknown };
  set_zoom_factor: { input: SetZoomFactorInput; output: null };
  get_zoom_factor: { input: Record<string, never>; output: unknown };
  enable_modern_window_style: { input: Record<string, unknown>; output: null };
  // duplicates
  get_duplicates: { input: GetDuplicatesInput; output: unknown };
  scan_duplicates: { input: ScanDuplicatesInput; output: unknown };
  get_duplicate_pairs: { input: GetDuplicatePairsInput; output: unknown };
  resolve_duplicate_pair: { input: ResolveDuplicatePairInput; output: unknown };
  get_duplicate_count: { input: Record<string, never>; output: unknown };
  get_duplicate_settings: { input: Record<string, never>; output: unknown };
  update_duplicate_settings: { input: UpdateDuplicateSettingsInput; output: unknown };
  // smart_folders
  reorder_smart_folders: { input: ReorderSmartFoldersInput; output: null };
  create_smart_folder: { input: CreateSmartFolderInput; output: unknown };
  update_smart_folder: { input: UpdateSmartFolderInput; output: unknown };
  delete_smart_folder: { input: DeleteSmartFolderInput; output: null };
  list_smart_folders: { input: Record<string, never>; output: unknown };
  query_smart_folder: { input: QuerySmartFolderInput; output: unknown };
  count_smart_folder: { input: CountSmartFolderInput; output: unknown };
}
