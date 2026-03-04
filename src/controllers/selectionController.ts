import { api } from '#desktop/api';
import {
  getOrStartSelectionSummary,
  invalidateSelectionSummary,
} from '../components/image-grid/metadataPrefetch';
import type { SelectionQuerySpec, SelectionSummary } from '../types/api';

/**
 * SelectionController — orchestration facade for virtual-selection summary
 * loading/invalidation and batch tag operations.
 */
export const SelectionController = {
  getOrStartSummary(spec: SelectionQuerySpec): Promise<SelectionSummary> {
    return getOrStartSelectionSummary(spec);
  },

  invalidateSummary(selectionKey?: string): void {
    invalidateSelectionSummary(selectionKey);
  },

  addTagsSelection(selection: SelectionQuerySpec, tagStrings: string[]): Promise<number> {
    return api.selection.addTags(selection, tagStrings);
  },

  removeTagsSelection(selection: SelectionQuerySpec, tagStrings: string[]): Promise<number> {
    return api.selection.removeTags(selection, tagStrings);
  },

  updateRatingSelection(selection: SelectionQuerySpec, rating: number | null): Promise<number> {
    return api.selection.updateRating(selection, rating);
  },

  setNotesSelection(selection: SelectionQuerySpec, notes: Record<string, string>): Promise<number> {
    return api.selection.setNotes(selection, notes);
  },

  setSourceUrlsSelection(selection: SelectionQuerySpec, urls: string[]): Promise<number> {
    return api.selection.setSourceUrls(selection, urls);
  },
};
