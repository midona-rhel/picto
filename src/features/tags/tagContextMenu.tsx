import {
  IconBookmark,
  IconBookmarks,
  IconCopy,
  IconSearch,
  IconStar,
  IconStarOff,
  IconX,
} from '@tabler/icons-react';
import type { MenuEntry } from '../../shared/ui/ContextMenu/ContextMenu';
import type { CanonicalNamespaceSummary } from '../../shared/types/canonical';
import { t } from '../../i18n';

export interface TagNameParts {
  namespace: string;
  subname: string;
}

export interface TagMenuTarget extends TagNameParts {
  tag_id: number;
}

export function tagNamespace(namespace: string): string | null {
  const normalized = namespace.trim().toLowerCase();
  return !normalized || normalized === 'general' ? null : normalized;
}

export function tagName(tag: TagNameParts): string {
  const namespace = tagNamespace(tag.namespace);
  return namespace ? `${namespace}:${tag.subname}` : tag.subname;
}

export function tagNameInGroup(tag: TagNameParts, namespace: string | null): string {
  return namespace ? `${namespace}:${tag.subname}` : tag.subname;
}

function copyText(value: string): void {
  const clipboard = (window as Window & { picto?: { clipboard?: { writeText?: (text: string) => void } } }).picto?.clipboard;
  if (clipboard?.writeText) clipboard.writeText(value);
  else void navigator.clipboard.writeText(value);
}

export function buildCommonTagContextEntries({
  tag,
  namespaces,
  starred,
  onFilter,
  onStarChange,
  onMoveToGroup,
  onRemove,
}: {
  tag: TagMenuTarget;
  namespaces: CanonicalNamespaceSummary[];
  starred: boolean;
  onFilter: (tag: { tag_id: number; name: string }) => void;
  onStarChange: (tag: string, starred: boolean) => void;
  onMoveToGroup?: (namespace: string | null) => void;
  onRemove?: () => void;
}): MenuEntry[] {
  const name = tagName(tag);
  const currentNamespace = tagNamespace(tag.namespace);
  const groupTargets = namespaces
    .map((item) => tagNamespace(item.name))
    .filter((namespace): namespace is string => namespace !== null && namespace !== currentNamespace)
    .filter((namespace, index, values) => values.indexOf(namespace) === index)
    .sort((left, right) => left.localeCompare(right));
  const entries: MenuEntry[] = [
    {
      label: t("Filter Items with This Tag"),
      icon: <IconSearch size={16} />,
      disabled: tag.tag_id <= 0,
      action: () => onFilter({ tag_id: tag.tag_id, name }),
    },
    {
      label: starred ? t("Remove from Starred") : t("Add to Starred"),
      icon: starred ? <IconStarOff size={16} /> : <IconStar size={16} />,
      action: () => onStarChange(name, !starred),
    },
  ];

  if (onMoveToGroup && groupTargets.length > 0) {
    entries.push({
      submenu: true,
      label: currentNamespace ? t("Move to Group…") : t("Add to Group…"),
      icon: <IconBookmarks size={16} />,
      children: groupTargets.map((namespace) => ({
        label: namespace,
        icon: <IconBookmark size={16} />,
        action: () => onMoveToGroup(namespace),
      })),
    });
  }
  if (onMoveToGroup && currentNamespace) {
    entries.push({
      label: t("Remove from this Group"),
      icon: <IconBookmark size={16} />,
      action: () => onMoveToGroup(null),
    });
  }

  entries.push(
    { separator: true },
    {
      label: t("Copy Tag"),
      icon: <IconCopy size={16} />,
      action: () => copyText(name),
    },
  );
  if (onRemove) {
    entries.push({
      label: t("Remove Tag"),
      icon: <IconX size={16} />,
      action: onRemove,
    });
  }
  return entries;
}
