import type { GridRuntimeAction } from './gridRuntimeReducer';
import type { GridDataState } from './gridDataState';

function uniqueByHash<T extends { hash: string }>(items: T[]): T[] {
  const seen = new Set<string>();
  const deduped: T[] = [];
  for (const item of items) {
    if (seen.has(item.hash)) continue;
    seen.add(item.hash);
    deduped.push(item);
  }
  return deduped;
}

function isDataAction(action: GridRuntimeAction): boolean {
  return action.type === 'SET_IMAGES'
    || action.type === 'APPEND_IMAGES'
    || action.type === 'FILTER_IMAGES'
    || action.type === 'SET_CURSOR'
    || action.type === 'SET_RESPONSE_TOTAL_COUNT'
    || action.type === 'SET_HAS_MORE'
    || action.type === 'SET_ERROR'
    || action.type === 'CLEAR_DATASET'
    || action.type === 'COMMIT_GEOMETRY'
    || action.type === 'SET_TRANSITION_STAGE';
}

export function gridDataReducer(state: GridDataState, action: GridRuntimeAction): GridDataState {
  if (!isDataAction(action)) return state;

  if (action.type === 'SET_TRANSITION_STAGE') {
    return { ...state, transitionStage: action.stage };
  }

  switch (action.type) {
    case 'SET_IMAGES':
      return { ...state, images: uniqueByHash(action.images) };

    case 'APPEND_IMAGES': {
      const merged = uniqueByHash([...state.images, ...action.images]);
      return {
        ...state,
        images: merged,
        hasMore: merged.length >= action.maxItems ? false : state.hasMore,
      };
    }

    case 'FILTER_IMAGES':
      return { ...state, images: state.images.filter(action.predicate) };

    case 'SET_CURSOR':
      return { ...state, defaultGridCursor: action.cursor, hasMore: action.hasMore };

    case 'SET_RESPONSE_TOTAL_COUNT':
      return { ...state, responseTotalCount: action.count };

    case 'SET_HAS_MORE':
      return { ...state, hasMore: action.hasMore };

    case 'SET_ERROR':
      return { ...state, error: action.error };

    case 'CLEAR_DATASET':
      return {
        ...state,
        images: [],
        defaultGridCursor: null,
        hasMore: false,
        responseTotalCount: null,
      };

    case 'COMMIT_GEOMETRY':
      return {
        ...state,
        displayViewMode: action.viewMode,
        displayTargetSize: action.targetSize,
        displayFolderId: action.folderId,
        displaySearchTags: action.searchTags,
        displayEmptyContext: action.emptyContext,
      };
  }

  return state;
}
