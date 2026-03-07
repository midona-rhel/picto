/**
 * Centralized API types — single source of truth for all backend command
 * request/response interfaces and event payloads.
 *
 * Split into focused modules to keep file-size budgets healthy.
 */

export * from './api/core';
export * from './api/events';

// Canonical sidebar definitions live in types/sidebar.ts.
export type {
  SidebarNodeKind,
  SidebarFreshness,
  SidebarNodeDto,
  SidebarTreeResponse,
} from './sidebar';
