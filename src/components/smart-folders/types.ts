export interface SmartRule {
  field: string;
  op: string;
  value?: string | number | boolean;
  value2?: string | number;
  values?: string[];
}

export interface SmartRuleGroup {
  match_mode: 'all' | 'any';
  negate: boolean;
  rules: SmartRule[];
}

export interface SmartFolderPredicate {
  groups: SmartRuleGroup[];
}

export interface SmartFolder {
  id?: string;
  name: string;
  icon?: string | null;
  color?: string | null;
  predicate: SmartFolderPredicate;
  sort_field?: string | null;
  sort_order?: string | null;
  created_at?: string | null;
  updated_at?: string | null;
}

/** Convert our flat SmartRule to the Rust tagged enum format for IPC.
 *  IMPORTANT: Must always send `null` (not undefined) for optional fields
 *  so serde can deserialize Option<T> in internally-tagged enums. */
export function ruleToRust(rule: SmartRule): Record<string, unknown> {
  const { field, op, value, value2, values } = rule;
  const result: Record<string, unknown> = { field, op };

  switch (field) {
    case 'tags':
    case 'color':
      result.values = values || [];
      break;
    case 'has_audio':
      result.value = Boolean(value);
      break;
    case 'rating':
    case 'file_size':
    case 'width':
    case 'height':
    case 'aspect_ratio':
    case 'duration':
    case 'view_count':
      result.value = Number(value) || 0;
      result.value2 = value2 != null ? Number(value2) : null;
      break;
    case 'date_imported':
      result.value = value != null ? String(value) : null;
      result.value2 = value2 != null ? String(value2) : null;
      break;
    default:
      result.value = String(value ?? '');
      break;
  }

  return result;
}

/** Convert predicate for IPC to backend */
export function predicateToRust(predicate: SmartFolderPredicate) {
  return {
    groups: predicate.groups.map((g) => ({
      match_mode: g.match_mode,
      negate: g.negate,
      rules: g.rules.map(ruleToRust),
    })),
  };
}

/** Convert a folder to the Rust-compatible SmartFolder struct for IPC.
 *  Rust expects `smart_folder_id` (i64) and `predicate_json` (String). */
export function folderToRust(folder: SmartFolder) {
  return {
    smart_folder_id: folder.id ? parseInt(folder.id, 10) : 0,
    name: folder.name,
    icon: folder.icon ?? null,
    color: folder.color ?? null,
    predicate_json: JSON.stringify(predicateToRust(folder.predicate)),
    sort_field: folder.sort_field ?? null,
    sort_order: folder.sort_order ?? null,
    created_at: folder.created_at ?? null,
    updated_at: folder.updated_at ?? null,
  };
}

export function createDefaultRule(): SmartRule {
  return { field: 'tags', op: 'include', values: [] };
}

export function createDefaultGroup(): SmartRuleGroup {
  return { match_mode: 'all', negate: false, rules: [createDefaultRule()] };
}
