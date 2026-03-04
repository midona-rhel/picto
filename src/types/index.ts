// Database types matching our planned SQLite schema

export interface Image {
  hash: string;
  filename: string;
  width: number;
  height: number;
  file_size: number;
  source?: string; // 'deviantart', 'manual', etc.
  source_url?: string;
  author?: string;
  title?: string;
  description?: string;
  rating: number; // 0-5 stars
  created_at: number;
  updated_at: number;
  source_created_at?: number;
  tags?: string[];
}

export interface Tag {
  id: number;
  name: string;
}

export interface Collection {
  id: number;
  name: string;
  description?: string;
  created_at: string;
}

// UI State types
export type ViewType = "images" | "collections" | "flows" | "dashboard" | "duplicates" | "tags" | "settings";

export type GridMode = "grid" | "list";

export type SortBy = "created_at" | "updated_at" | "rating" | "filename";

export type SortOrder = "asc" | "desc";

// Plugin system types
export interface PluginConfig {
  id: string;
  name: string;
  enabled: boolean;
  settings: Record<string, any>;
}

export interface DeviantArtConfig extends PluginConfig {
  id: "deviantart";
  settings: {
    keywords: string[];
    followedArtists: string[];
    maxImagesPerRequest: number;
    requestDelay: number;
  };
}

// Image import and processing
export interface ImportedImage {
  filename: string;
  filepath: string;
  thumbnailPath?: string;
  metadata?: {
    width: number;
    height: number;
    size: number;
    format: string;
  };
}

// Review queue types
export interface ReviewQueueItem {
  id: number;
  hash: string;
  filename: string;
  width: number;
  height: number;
  file_size: number;
  source: string;
  source_url?: string;
  author?: string;
  title?: string;
  description?: string;
  suggested_tags: string[];
  additional_metadata: Record<string, string>;
  created_at: number;
}

// Search and filter types
export interface SearchFilters {
  query?: string;
  tags?: string[];
  collections?: number[];
  rating?: {
    min: number;
    max: number;
  };
  source?: string;
  dateRange?: {
    from: Date;
    to: Date;
  };
}