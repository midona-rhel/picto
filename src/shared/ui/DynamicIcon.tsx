/**
 * Dynamic icon resolver — maps icon name strings to Tabler icon components.
 *
 * Used by sidebar rows for folder/smart-folder icons that are user-configurable.
 * System scope icons are used directly as JSX in the feature root.
 */

import {
  IconFolder,
  IconFolderOpen,
  IconStar,
  IconHeart,
  IconBookmark,
  IconFlame,
  IconBolt,
  IconCamera,
  IconPalette,
  IconMusic,
  IconVideo,
  IconPhoto,
  IconDownload,
  IconCloud,
  IconWorld,
  IconCode,
  IconBrush,
  IconPencil,
  IconEye,
  IconDiamond,
  IconCrown,
  IconAward,
  IconRocket,
  IconTarget,
  IconFlag,
  IconTag,
  IconSettings,
  IconHome,
  IconUser,
  IconUsers,
  IconMail,
  IconCalendar,
  IconClock,
  IconSearch,
  IconFilter,
  IconList,
  IconGrid3x3 as IconGrid,
  IconArchive,
  IconBox,
  IconPackage,
  IconShield,
  IconLock,
  IconKey,
  IconLink,
  IconPaperclip,
  IconFile,
  IconFiles,
  IconClipboard,
  type Icon as TablerIcon,
} from '@tabler/icons-react';

const ICON_MAP: Record<string, TablerIcon> = {
  IconFolder,
  IconFolderOpen,
  IconStar,
  IconHeart,
  IconBookmark,
  IconFlame,
  IconBolt,
  IconCamera,
  IconPalette,
  IconMusic,
  IconVideo,
  IconPhoto,
  IconDownload,
  IconCloud,
  IconWorld,
  IconCode,
  IconBrush,
  IconPencil,
  IconEye,
  IconDiamond,
  IconCrown,
  IconAward,
  IconRocket,
  IconTarget,
  IconFlag,
  IconTag,
  IconSettings,
  IconHome,
  IconUser,
  IconUsers,
  IconMail,
  IconCalendar,
  IconClock,
  IconSearch,
  IconFilter,
  IconList,
  IconGrid,
  IconArchive,
  IconBox,
  IconPackage,
  IconShield,
  IconLock,
  IconKey,
  IconLink,
  IconPaperclip,
  IconFile,
  IconFiles,
  IconClipboard,
  // Legacy aliases
  palette: IconPalette,
  download: IconDownload,
  star: IconStar,
};

interface DynamicIconProps {
  name: string;
  size?: number;
  color?: string | null;
  stroke?: number;
  filled?: boolean;
}

export function DynamicIcon({ name, size = 16, color, stroke = 1.2, filled }: DynamicIconProps) {
  const Icon = ICON_MAP[name] ?? IconFolder;
  if (filled) {
    return (
      <Icon
        size={size}
        stroke={stroke}
        fill={color ?? 'currentColor'}
        fillOpacity={0.15}
        color={color ?? undefined}
      />
    );
  }
  return <Icon size={size} stroke={stroke} color={color ?? undefined} />;
}
