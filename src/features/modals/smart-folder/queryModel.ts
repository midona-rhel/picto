import { getTagsById, getTagsPaginated } from '../../../platform/tagApi';
import { hexToLab, labToHex } from '../../../shared/lib/labColor';
import type {
  FilterClause,
  FilterExpr,
  Rating,
  SmartFolderPredicate,
  SmartFolderPredicateGroup,
  SmartFolderPredicateRule,
} from '../../../shared/types/canonical';

const RATINGS: Rating[] = ['unrated', 'one', 'two', 'three', 'four', 'five'];
const DAY_MS = 86_400_000;
const FILE_SIZE_SCALE: Record<string, number> = {
  B: 1,
  KB: 1024,
  MB: 1024 * 1024,
  GB: 1024 * 1024 * 1024,
};

function wrapNot(expression: FilterExpr, negate: boolean): FilterExpr {
  return negate ? { kind: 'not', value: expression } : expression;
}

function numberRange(rule: SmartFolderPredicateRule, scale = 1) {
  const value = Math.max(0, Math.round(Number(rule.value ?? 0) * scale));
  const value2 = Math.max(0, Math.round(Number(rule.value2 ?? rule.value ?? 0) * scale));
  switch (rule.op) {
    case 'eq': return { minimum: value, maximum: value, negate: false };
    case 'neq': return { minimum: value, maximum: value, negate: true };
    case 'gt': return { minimum: value + 1, maximum: null, negate: false };
    case 'gte': return { minimum: value, maximum: null, negate: false };
    case 'lt': return { minimum: null, maximum: Math.max(0, value - 1), negate: false };
    case 'lte': return { minimum: null, maximum: value, negate: false };
    case 'between': return { minimum: Math.min(value, value2), maximum: Math.max(value, value2), negate: false };
    default: throw new Error(`Unsupported numeric operator: ${rule.op}`);
  }
}

function dateRange(rule: SmartFolderPredicateRule) {
  const parse = (value: unknown) => {
    const timestamp = Date.parse(String(value ?? ''));
    if (!Number.isFinite(timestamp)) throw new Error('Smart-folder date is invalid.');
    return timestamp;
  };
  const value = parse(rule.value);
  const value2 = parse(rule.value2 ?? rule.value);
  switch (rule.op) {
    case 'eq': return { minimum_ms: value, maximum_ms: value + DAY_MS - 1 };
    case 'gt': return { minimum_ms: value + DAY_MS, maximum_ms: null };
    case 'gte': return { minimum_ms: value, maximum_ms: null };
    case 'lt': return { minimum_ms: null, maximum_ms: value - 1 };
    case 'lte': return { minimum_ms: null, maximum_ms: value + DAY_MS - 1 };
    case 'between': return {
      minimum_ms: Math.min(value, value2),
      maximum_ms: Math.max(value, value2) + DAY_MS - 1,
    };
    default: throw new Error(`Unsupported date operator: ${rule.op}`);
  }
}

async function resolveTagIds(names: string[]): Promise<number[]> {
  return Promise.all(names.map(async (name) => {
    const separator = name.indexOf(':');
    const namespace = separator < 0 ? '' : name.slice(0, separator);
    const subname = separator < 0 ? name : name.slice(separator + 1);
    const page = await getTagsPaginated({
      namespace: namespace || null,
      search: subname,
      limit: 100,
    });
    const match = page.tags.find((tag) => (
      tag.namespace === namespace && tag.subname === subname
    ));
    if (!match) throw new Error(`Smart folder references a tag that does not exist: ${name}`);
    return match.tag_id;
  }));
}

async function compileRule(rule: SmartFolderPredicateRule): Promise<FilterExpr> {
  let clause: FilterClause;
  let negate = false;
  switch (rule.field) {
    case 'tags': {
      const tagIds = await resolveTagIds(rule.values ?? []);
      clause = {
        clause: 'tags',
        tag_ids: tagIds,
        mode: rule.op === 'include_all' ? 'all' : 'any',
      };
      negate = rule.op === 'do_not_include';
      break;
    }
    case 'file_type': {
      const value = String(rule.value ?? '');
      clause = {
        clause: 'mime',
        values: value.includes('/') ? [value] : [],
        families: value.includes('/') ? [] : [value],
      };
      negate = rule.op === 'is_not';
      break;
    }
    case 'rating': {
      const range = numberRange(rule);
      const ratings = RATINGS.filter((_, index) => (
        (range.minimum == null || index >= range.minimum)
        && (range.maximum == null || index <= range.maximum)
      ));
      clause = { clause: 'ratings', ratings };
      negate = range.negate;
      break;
    }
    case 'file_size': {
      const range = numberRange(rule, FILE_SIZE_SCALE[rule.unit ?? 'MB'] ?? FILE_SIZE_SCALE.MB);
      clause = { clause: 'total_size', minimum_bytes: range.minimum, maximum_bytes: range.maximum };
      negate = range.negate;
      break;
    }
    case 'date_added': clause = { clause: 'imported_at', ...dateRange(rule) }; break;
    case 'date_created': clause = { clause: 'captured_at', ...dateRange(rule) }; break;
    case 'date_modified': clause = { clause: 'modified_at', ...dateRange(rule) }; break;
    case 'width': {
      const range = numberRange(rule);
      clause = { clause: 'width', minimum: range.minimum, maximum: range.maximum };
      negate = range.negate;
      break;
    }
    case 'height': {
      const range = numberRange(rule);
      clause = { clause: 'height', minimum: range.minimum, maximum: range.maximum };
      negate = range.negate;
      break;
    }
    case 'duration': {
      const range = numberRange(rule, 1000);
      clause = { clause: 'duration', minimum_ms: range.minimum, maximum_ms: range.maximum };
      negate = range.negate;
      break;
    }
    case 'name': clause = { clause: 'text', field: 'name', query: String(rule.value ?? '') }; break;
    case 'notes':
      if (rule.op === 'is_empty' || rule.op === 'is_not_empty') {
        clause = { clause: 'notes_present', present: rule.op === 'is_not_empty' };
      } else {
        clause = { clause: 'text', field: 'notes', query: String(rule.value ?? '') };
      }
      break;
    case 'source_url':
      if (rule.op === 'is_empty' || rule.op === 'is_not_empty') {
        clause = { clause: 'source_urls_present', present: rule.op === 'is_not_empty' };
      } else {
        clause = { clause: 'text', field: 'source_url', query: String(rule.value ?? '') };
      }
      break;
    case 'color': {
      const value = rule.values?.[0];
      if (!value) throw new Error('Smart-folder color is empty.');
      clause = { clause: 'color', color: hexToLab(value), delta_e: 12 };
      break;
    }
    default: throw new Error(`Unsupported smart-folder field: ${rule.field}`);
  }
  return wrapNot({ kind: 'clause', value: clause }, negate);
}

export async function compileSmartFolderPredicate(
  predicate: SmartFolderPredicate,
): Promise<FilterExpr> {
  const groups = await Promise.all(predicate.groups.map(async (group) => {
    const rules = await Promise.all(group.rules.map(compileRule));
    const expression: FilterExpr = group.match_mode === 'any'
      ? { kind: 'any', value: rules }
      : { kind: 'all', value: rules };
    return wrapNot(expression, !!group.negate);
  }));
  return { kind: 'all', value: groups };
}

function rangeRule(
  field: string,
  minimum: number | null,
  maximum: number | null,
  scale = 1,
): SmartFolderPredicateRule {
  if (minimum != null && maximum != null && minimum === maximum) {
    return { field, op: 'eq', value: minimum / scale };
  }
  if (minimum != null && maximum != null) {
    return { field, op: 'between', value: minimum / scale, value2: maximum / scale };
  }
  if (minimum != null) return { field, op: 'gte', value: minimum / scale };
  return { field, op: 'lte', value: (maximum ?? 0) / scale };
}

function dateRule(field: string, minimum: number | null, maximum: number | null): SmartFolderPredicateRule {
  const date = (value: number) => new Date(value).toISOString().slice(0, 10);
  if (minimum != null && maximum != null) {
    if (maximum - minimum < DAY_MS) return { field, op: 'eq', value: date(minimum) };
    return { field, op: 'between', value: date(minimum), value2: date(maximum) };
  }
  if (minimum != null) return { field, op: 'gte', value: date(minimum) };
  return { field, op: 'lte', value: date(maximum ?? 0) };
}

function ruleFromClause(clause: FilterClause, tagNames: Map<number, string>): SmartFolderPredicateRule {
  switch (clause.clause) {
    case 'tags': return {
      field: 'tags',
      op: clause.mode === 'all' ? 'include_all' : 'include_any',
      values: clause.tag_ids.map((id) => tagNames.get(id) ?? String(id)),
    };
    case 'mime': {
      const value = clause.values[0] ?? clause.families[0] ?? 'image';
      return { field: 'file_type', op: 'is', value };
    }
    case 'ratings': {
      const indexes = clause.ratings.map((rating) => RATINGS.indexOf(rating)).sort((a, b) => a - b);
      return rangeRule('rating', indexes[0] ?? 0, indexes[indexes.length - 1] ?? 0);
    }
    case 'imported_at': return dateRule('date_added', clause.minimum_ms, clause.maximum_ms);
    case 'captured_at': return dateRule('date_created', clause.minimum_ms, clause.maximum_ms);
    case 'modified_at': return dateRule('date_modified', clause.minimum_ms, clause.maximum_ms);
    case 'width': return rangeRule('width', clause.minimum, clause.maximum);
    case 'height': return rangeRule('height', clause.minimum, clause.maximum);
    case 'duration': return rangeRule('duration', clause.minimum_ms, clause.maximum_ms, 1000);
    case 'total_size': return {
      ...rangeRule('file_size', clause.minimum_bytes, clause.maximum_bytes, 1024 * 1024),
      unit: 'MB',
    };
    case 'notes_present': return { field: 'notes', op: clause.present ? 'is_not_empty' : 'is_empty' };
    case 'source_urls_present': return { field: 'source_url', op: clause.present ? 'is_not_empty' : 'is_empty' };
    case 'color': return { field: 'color', op: 'contains', values: [labToHex(clause.color) ?? '#000000'] };
    case 'text': {
      if (clause.field === 'global') throw new Error('Global text rules are not exposed by the smart-folder editor.');
      return {
      field: clause.field === 'source_url' ? 'source_url' : clause.field,
      op: 'contains',
      value: clause.query,
      };
    }
    case 'folders': throw new Error('Folder rules are not exposed by the smart-folder editor.');
  }
}

function collectTagIds(expression: FilterExpr, target: Set<number>) {
  if (expression.kind === 'clause' && expression.value.clause === 'tags') {
    expression.value.tag_ids.forEach((id) => target.add(id));
  } else if (expression.kind === 'all' || expression.kind === 'any') {
    expression.value.forEach((child) => collectTagIds(child, target));
  } else if (expression.kind === 'not') {
    collectTagIds(expression.value, target);
  }
}

function unwrapGroup(expression: FilterExpr, tagNames: Map<number, string>): SmartFolderPredicateGroup {
  const negated = expression.kind === 'not';
  const inner = negated ? expression.value : expression;
  const children = inner.kind === 'all' || inner.kind === 'any' ? inner.value : [inner];
  const rules = children.map((child) => {
    const childNegated = child.kind === 'not';
    const clauseExpr = childNegated ? child.value : child;
    if (clauseExpr.kind !== 'clause') throw new Error('Nested smart-folder expression cannot be edited.');
    const rule = ruleFromClause(clauseExpr.value, tagNames);
    if (childNegated) {
      if (rule.field === 'tags') rule.op = 'do_not_include';
      else if (rule.field === 'file_type') rule.op = 'is_not';
      else if (rule.op === 'eq') rule.op = 'neq';
      else throw new Error('Negated smart-folder rule cannot be edited.');
    }
    return rule;
  });
  return {
    match_mode: inner.kind === 'any' ? 'any' : 'all',
    negate: negated,
    rules,
  };
}

export async function editorPredicateFromFilter(filter: FilterExpr): Promise<SmartFolderPredicate> {
  const tagIds = new Set<number>();
  collectTagIds(filter, tagIds);
  const tags = tagIds.size ? await getTagsById([...tagIds]) : [];
  const tagNames = new Map(tags.map((tag) => [
    tag.tag_id,
    tag.namespace ? `${tag.namespace}:${tag.subname}` : tag.subname,
  ]));
  const expressions = filter.kind === 'all' ? filter.value : [filter];
  return { groups: expressions.map((expression) => unwrapGroup(expression, tagNames)) };
}
