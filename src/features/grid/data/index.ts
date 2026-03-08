export {
  prefetchMetadata,
  prefetchMetadataBatch,
  getMetadata,
  invalidateMetadata,
  invalidateManyMetadata,
  pinMetadata,
  unpinMetadata,
  getOrStartSelectionSummary,
  invalidateSelectionSummary,
  getMetadataCacheDebugStats,
} from '../metadataPrefetch';

export type {
  EntityAllMetadata,
  ResolvedTagInfo,
  SelectionQuerySpec,
  SelectionSummary,
} from '../metadataPrefetch';

export { cleanupMediaCache } from '../enhancedMediaCache';
