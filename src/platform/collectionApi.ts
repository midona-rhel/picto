import { invoke } from './ipc';

export interface CollectionSummary {
  id: number;
  name: string;
  image_count: number;
  total_size_bytes: number;
}

export function getCollectionSummary(collectionId: number): Promise<CollectionSummary> {
  return invoke<CollectionSummary>('get_collection_summary', { id: collectionId });
}

export function createCollection(name: string, hashes: string[]): Promise<number> {
  return invoke<number>('create_collection', { name, hashes });
}

export function addCollectionMembers(collectionId: number, hashes: string[]): Promise<number> {
  return invoke<number>('add_collection_members', { id: collectionId, hashes });
}

export function removeCollectionMembers(collectionId: number, hashes: string[]): Promise<number> {
  return invoke<number>('remove_collection_members', { id: collectionId, hashes });
}

export function reorderCollectionMembers(collectionId: number, orderedHashes: string[]): Promise<void> {
  return invoke<void>('reorder_collection_members', { id: collectionId, hashes: orderedHashes });
}

export function splitCollection(collectionId: number): Promise<string[]> {
  return invoke<string[]>('split_collection', { id: collectionId });
}
