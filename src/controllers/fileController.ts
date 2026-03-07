import { api, copyFileToClipboard, copyImageToClipboard, reverseImageSearch, type ReverseImageEngine } from '#desktop/api';
import type { TagDisplay } from '../shared/types/api';

/**
 * FileController — frontend facade for single-file commands.
 */
export const FileController = {
  openDefault(hash: string): Promise<void> {
    return api.file.openDefault(hash);
  },

  openInNewWindow(hash: string, imgWidth?: number | null, imgHeight?: number | null): Promise<void> {
    return api.file.openInNewWindow(hash, imgWidth, imgHeight);
  },

  revealInFolder(hash: string): Promise<void> {
    return api.file.revealInFolder(hash);
  },

  setSourceUrls(hash: string, urls: string[]): Promise<void> {
    return api.file.setSourceUrls(hash, urls);
  },

  updateRating(hash: string, rating: number): Promise<void> {
    return api.file.updateRating(hash, rating);
  },

  removeTags(hash: string, tagStrings: string[]): Promise<void> {
    return api.tags.remove(hash, tagStrings);
  },

  addTags(hash: string, tagStrings: string[]): Promise<void> {
    return api.tags.add(hash, tagStrings) as Promise<void>;
  },

  resolveFilePath(hash: string): Promise<string> {
    return api.file.resolvePath(hash);
  },

  getFileTags(hash: string): Promise<TagDisplay[]> {
    return api.tags.getForFile(hash);
  },

  setFileName(hash: string, name: string | null): Promise<void> {
    return api.file.setName(hash, name);
  },

  openExternalUrl(url: string): Promise<void> {
    return api.os.openExternalUrl(url);
  },

  /** Copy the actual file to the system clipboard. */
  async copyToClipboard(hash: string): Promise<void> {
    const filePath = await this.resolveFilePath(hash);
    await copyFileToClipboard(filePath);
  },

  /** Copy the thumbnail image to the system clipboard. */
  async copyThumbnailToClipboard(hash: string): Promise<void> {
    const thumbPath = await api.file.resolveThumbnailPath(hash);
    await copyImageToClipboard(thumbPath);
  },

  /** Upload image to a reverse image search engine and open results in browser. */
  async searchByImage(hash: string, engine: ReverseImageEngine): Promise<void> {
    const filePath = await this.resolveFilePath(hash);
    await reverseImageSearch(filePath, engine);
  },

  /** Regenerate thumbnail for a single file. */
  regenerateThumbnail(hash: string) {
    return api.file.regenerateThumbnail(hash);
  },

  /** Re-run dominant color extraction for a single file. */
  reanalyzeColors(hash: string) {
    return api.file.reanalyzeColors(hash);
  },

  /** Regenerate thumbnails for multiple files. */
  regenerateThumbnailsBatch(hashes: string[]) {
    return api.file.regenerateThumbnailsBatch(hashes);
  },
};
