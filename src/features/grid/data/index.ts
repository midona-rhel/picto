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
} from '../../../components/image-grid/metadataPrefetch';

export type {
  EntityAllMetadata,
  ResolvedTagInfo,
  SelectionQuerySpec,
  SelectionSummary,
} from '../../../components/image-grid/metadataPrefetch';

export { cleanupMediaCache } from '../../../components/image-grid/enhancedMediaCache';
