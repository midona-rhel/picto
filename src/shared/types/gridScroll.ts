export interface GridScrollPosition {
  /** Exact offset is retained as a fallback before a full extent can be estimated. */
  scrollTop: number;
  /** Position within the scrollable range, from the start (0) to the end (1). */
  progress: number;
}
