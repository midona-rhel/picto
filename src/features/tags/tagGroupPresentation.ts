import {
  IconBook,
  IconBookmark,
  IconPaw,
  IconPencil,
  IconSettings,
  IconStar,
  IconUser,
  type Icon,
} from '@tabler/icons-react';

type GroupPresentation = {
  color: [number, number, number];
  darkText: string;
  lightText: string;
  icon: Icon;
  order: number;
};

const DEFAULT_GROUP: GroupPresentation = {
  color: [189, 190, 192],
  darkText: '#f8f9fb',
  lightText: '#2c2f32',
  icon: IconBookmark,
  order: 10,
};

const GROUPS: Record<string, GroupPresentation> = {
  creator: { color: [255, 102, 103], darkText: '#f8e7e6', lightText: '#513636', icon: IconPencil, order: 0 },
  series: { color: [196, 153, 255], darkText: '#f2eef9', lightText: '#443b52', icon: IconBook, order: 1 },
  character: { color: [48, 209, 89], darkText: '#e4f5e9', lightText: '#34403c', icon: IconUser, order: 2 },
  species: { color: [0, 170, 255], darkText: '#dff1f9', lightText: '#3c4e5a', icon: IconPaw, order: 3 },
  rating: { color: [255, 214, 10], darkText: '#f8f5e1', lightText: '#4a4335', icon: IconStar, order: 4 },
  meta: { color: [189, 190, 192], darkText: '#f8f9fb', lightText: '#2c2f32', icon: IconSettings, order: 5 },
  general: DEFAULT_GROUP,
  '': DEFAULT_GROUP,
};

export function tagGroupPresentation(namespace: string | null | undefined): GroupPresentation {
  return GROUPS[(namespace ?? '').toLowerCase()] ?? DEFAULT_GROUP;
}

export function tagGroupColor(namespace: string | null | undefined): string {
  const [red, green, blue] = tagGroupPresentation(namespace).color;
  return `rgb(${red}, ${green}, ${blue})`;
}

export function tagGroupOrder(namespace: string): number {
  return tagGroupPresentation(namespace).order;
}

export function tagGroupTextColor(namespace: string | null | undefined, scheme: 'dark' | 'light'): string {
  const presentation = tagGroupPresentation(namespace);
  return scheme === 'dark' ? presentation.darkText : presentation.lightText;
}
