/**
 * Centralized z-index scale. Local stacking contexts (z-index: 1, 2, 3)
 * within components don't need these — only cross-component layers.
 */
export const Z = {
  navigator: 5,
  detailView: 10,
  overlay: 1000,
  quickLook: 2000,
  contextBackdrop: 9998,
  contextMenu: 9999,
  toolbar: 10000,
} as const;
