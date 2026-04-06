/**
 * Platform API facade.
 *
 * New code should prefer the smaller domain modules in `src/platform/**`.
 * This file remains as a compatibility surface for controllers that have not
 * been repointed yet.
 */

export * from './entityApi';
export * from './tagApi';
export * from './collectionApi';
export * from './sidebarApi';
export * from './folderApi';
export * from './smartFolderApi';
export * from './subscriptionApi';
export * from './aiTaggerApi';
export * from './shellApi';
export * from './settingsApi';
