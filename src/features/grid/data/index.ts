export {
  prefetchMetadata,
  prefetchMetadataBatch,
  getMetadata,
  noteMetadataChanged,
  noteManyMetadataChanged,
  pinMetadata,
  unpinMetadata,
  getOrStartSelectionSummary,
  noteSelectionSummaryChanged,
  getMetadataCacheDebugStats,
} from '../metadataPrefetch';

export type {
  EntityAllMetadata,
  ResolvedTagInfo,
  SelectionQuerySpec,
  SelectionSummary,
} from '../metadataPrefetch';
