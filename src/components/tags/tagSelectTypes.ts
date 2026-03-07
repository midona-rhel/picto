export type TagFilterLogicMode = 'OR' | 'AND' | 'EQUAL';

export interface TagSelectPanelProps {
  anchorEl?: HTMLElement | null;
  mode?: 'anchored' | 'modal';
  title?: string;
  selectedTags: string[];
  excludedTags?: string[];
  logicMode?: TagFilterLogicMode;
  onToggle: (tag: string, added: boolean) => void;
  onExclude?: (tag: string) => void;
  onExcludedTagsChange?: (tags: string[]) => void;
  onLogicChange?: (mode: TagFilterLogicMode) => void;
  onClose: () => void;
}
