export interface SitePluginInfo {
  id: string;
  name: string;
  domain: string;
  auth_supported?: boolean;
  auth_required_for_full_access?: boolean;
}

export interface SubscriptionQueryInfo {
  id: string;
  query_text: string;
  display_name: string | null;
  paused: boolean;
  last_check_time: string | null;
  files_found: number;
  last_seen_id: string | null;
}

export interface SubInfo {
  id: string;
  name: string;
  site_id?: string;
  site_plugin_id?: string;
  paused: boolean;
  flow_id: string | null;
  initial_file_limit: number;
  periodic_file_limit: number;
  created_at: string;
  total_files: number;
  queries: SubscriptionQueryInfo[];
}

export interface FlowInfo {
  id: string;
  name: string;
  schedule: string;
  created_at: string;
  total_files: number;
  subscriptions: SubInfo[];
}

export interface FlowExecutionSummary {
  added: number;
  skipped_duplicate: number;
  skipped_error: number;
  errors?: string[];
  method?: string;
}

export type FlowResultEntry = FlowExecutionSummary & { error?: string };

export interface FlowsWorkingProps {
  flowId?: string | null;
  lastResults: Record<string, FlowResultEntry>;
  onLastResultsChange: (results: Record<string, FlowResultEntry>) => void;
  onOpenCreateModal?: () => void;
  showHeader?: boolean;
  layoutMode?: 'grid' | 'list';
  headerTitle?: string;
  refreshToken?: number;
}

export interface SubProgress {
  filesDownloaded: number;
  filesSkipped: number;
  pagesFetched: number;
  statusText: string;
}

export const SCHEDULE_OPTIONS = [
  { value: 'manual', label: 'Manual' },
  { value: 'daily', label: 'Daily' },
  { value: 'weekly', label: 'Weekly' },
  { value: 'monthly', label: 'Monthly' },
];
