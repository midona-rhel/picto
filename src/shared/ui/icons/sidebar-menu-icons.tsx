/**
 * Custom sidebar context menu icons.
 *
 * Each icon accepts the same props as Tabler icons (size, stroke, color)
 * with the same defaults (size=24, stroke=2, color=currentColor).
 */

interface IconProps {
  size?: number;
  stroke?: number;
  color?: string;
  className?: string;
}

function defaults(props: IconProps) {
  return {
    size: props.size ?? 24,
    stroke: props.stroke ?? 1.5,
    color: props.color ?? 'currentColor',
  };
}

/** New Subfolder — large folder with small folder nested at bottom-right. */
export function IconNewSubfolder(props: IconProps) {
  const { size, stroke, color } = defaults(props);
  return (
    <svg xmlns="http://www.w3.org/2000/svg" width={size} height={size} viewBox="0 0 24 24"
      fill="none" stroke={color} strokeWidth={stroke}
      strokeLinecap="round" strokeLinejoin="round" className={props.className}>
      <path d="M8 19h-3a2 2 0 0 1 -2 -2v-11a2 2 0 0 1 2 -2h4l3 3h7a2 2 0 0 1 2 2v2" />
      {/* Simple standard folder */}
      <path d="M13 13h3l1.5 1.5h3.5a1 1 0 0 1 1 1v5a1 1 0 0 1 -1 1h-8a1 1 0 0 1 -1 -1v-6.5a1 1 0 0 1 1 -1z" />
    </svg>
  );
}

/** Rename — Tabler forms icon with the two center dots removed. */
export function IconRename(props: IconProps) {
  const { size, stroke, color } = defaults(props);
  return (
    <svg xmlns="http://www.w3.org/2000/svg" width={size} height={size} viewBox="0 0 24 24"
      fill="none" stroke={color} strokeWidth={stroke}
      strokeLinecap="round" strokeLinejoin="round" className={props.className}>
      <path d="M12 3a3 3 0 0 0 -3 3v12a3 3 0 0 0 3 3" />
      <path d="M6 3a3 3 0 0 1 3 3v12a3 3 0 0 1 -3 3" />
      <path d="M13 7h7a1 1 0 0 1 1 1v8a1 1 0 0 1 -1 1h-7" />
      <path d="M5 7h-1a1 1 0 0 0 -1 1v8a1 1 0 0 0 1 1h1" />
    </svg>
  );
}

/** Sort — descending lines with arrow. */
export function IconSort(props: IconProps) {
  const { size, stroke, color } = defaults(props);
  return (
    <svg xmlns="http://www.w3.org/2000/svg" width={size} height={size} viewBox="0 0 24 24"
      fill="none" stroke={color} strokeWidth={stroke}
      strokeLinecap="round" strokeLinejoin="round" className={props.className}>
      <path d="M4 6h7" />
      <path d="M4 12h7" />
      <path d="M4 18h4" />
      <path d="M15 9l3 -3l3 3" />
      <path d="M18 6v12" />
    </svg>
  );
}

/** Expand — square with plus inside. */
export function IconExpand(props: IconProps) {
  const { size, stroke, color } = defaults(props);
  return (
    <svg xmlns="http://www.w3.org/2000/svg" width={size} height={size} viewBox="0 0 24 24"
      fill="none" stroke={color} strokeWidth={stroke}
      strokeLinecap="round" strokeLinejoin="round" className={props.className}>
      <path d="M3 5a2 2 0 0 1 2 -2h14a2 2 0 0 1 2 2v14a2 2 0 0 1 -2 2h-14a2 2 0 0 1 -2 -2v-14" />
      <path d="M9 12h6" />
      <path d="M12 9v6" />
    </svg>
  );
}

/** Collapse — square with minus inside. */
export function IconCollapse(props: IconProps) {
  const { size, stroke, color } = defaults(props);
  return (
    <svg xmlns="http://www.w3.org/2000/svg" width={size} height={size} viewBox="0 0 24 24"
      fill="none" stroke={color} strokeWidth={stroke}
      strokeLinecap="round" strokeLinejoin="round" className={props.className}>
      <path d="M3 5a2 2 0 0 1 2 -2h14a2 2 0 0 1 2 2v14a2 2 0 0 1 -2 2h-14a2 2 0 0 1 -2 -2v-14" />
      <path d="M9 12h6" />
    </svg>
  );
}

/** Expand/Collapse All — two overlapping squares (mirrored duplicate) with plus/minus. */
export function IconExpandAll(props: IconProps) {
  const { size, stroke, color } = defaults(props);
  return (
    <svg xmlns="http://www.w3.org/2000/svg" width={size} height={size} viewBox="0 0 24 24"
      fill="none" stroke={color} strokeWidth={stroke}
      strokeLinecap="round" strokeLinejoin="round" className={props.className}>
      {/* Back square (top-left) */}
      <path d="M4.012 16.737a2.005 2.005 0 0 1 -1.012 -1.737v-10c0 -1.1 .9 -2 2 -2h10c.75 0 1.158 .385 1.5 1" />
      {/* Front square (bottom-right) */}
      <path d="M7 7m0 2.667a2.667 2.667 0 0 1 2.667 -2.667h8.666a2.667 2.667 0 0 1 2.667 2.667v8.666a2.667 2.667 0 0 1 -2.667 2.667h-8.666a2.667 2.667 0 0 1 -2.667 -2.667z" />
      {/* Plus */}
      <path d="M11 14h6" />
      <path d="M14 11v6" />
    </svg>
  );
}

/** Collapse All — two overlapping squares with minus. */
export function IconCollapseAll(props: IconProps) {
  const { size, stroke, color } = defaults(props);
  return (
    <svg xmlns="http://www.w3.org/2000/svg" width={size} height={size} viewBox="0 0 24 24"
      fill="none" stroke={color} strokeWidth={stroke}
      strokeLinecap="round" strokeLinejoin="round" className={props.className}>
      {/* Back square (top-left) */}
      <path d="M4.012 16.737a2.005 2.005 0 0 1 -1.012 -1.737v-10c0 -1.1 .9 -2 2 -2h10c.75 0 1.158 .385 1.5 1" />
      {/* Front square (bottom-right) */}
      <path d="M7 7m0 2.667a2.667 2.667 0 0 1 2.667 -2.667h8.666a2.667 2.667 0 0 1 2.667 2.667v8.666a2.667 2.667 0 0 1 -2.667 2.667h-8.666a2.667 2.667 0 0 1 -2.667 -2.667z" />
      {/* Minus */}
      <path d="M11 14h6" />
    </svg>
  );
}

/** Auto Tags — folder with bookmark in bottom-right. */
export function IconAutoTags(props: IconProps) {
  const { size, stroke, color } = defaults(props);
  return (
    <svg xmlns="http://www.w3.org/2000/svg" width={size} height={size} viewBox="0 0 24 24"
      fill="none" stroke={color} strokeWidth={stroke}
      strokeLinecap="round" strokeLinejoin="round" className={props.className}>
      <path d="M9 19h-4a2 2 0 0 1 -2 -2v-11a2 2 0 0 1 2 -2h4l3 3h7a2 2 0 0 1 2 2" />
      {/* Bookmark */}
      <path d="M14 13v8l3.5 -2.5l3.5 2.5v-8h-7z" />
    </svg>
  );
}

/** Watch Folder — folder with eye in bottom-right. */
export function IconWatchFolder(props: IconProps) {
  const { size, stroke, color } = defaults(props);
  return (
    <svg xmlns="http://www.w3.org/2000/svg" width={size} height={size} viewBox="0 0 24 24"
      fill="none" stroke={color} strokeWidth={stroke}
      strokeLinecap="round" strokeLinejoin="round" className={props.className}>
      <path d="M8 19h-3a2 2 0 0 1 -2 -2v-11a2 2 0 0 1 2 -2h4l3 3h7a2 2 0 0 1 2 2v2" />
      {/* Eye */}
      <path d="M11 17.5c1.5 -2.5 3.5 -3.5 6 -3.5s4.5 1 6 3.5c-1.5 2.5 -3.5 3.5 -6 3.5s-4.5 -1 -6 -3.5z" />
      <circle cx="17" cy="17.5" r="1.5" />
    </svg>
  );
}

/** Change Icon — grid of four squares. */
export function IconChangeIcon(props: IconProps) {
  const { size, stroke, color } = defaults(props);
  return (
    <svg xmlns="http://www.w3.org/2000/svg" width={size} height={size} viewBox="0 0 24 24"
      fill="none" stroke={color} strokeWidth={stroke}
      strokeLinecap="round" strokeLinejoin="round" className={props.className}>
      <rect x="4" y="4" width="6" height="6" rx="1" />
      <rect x="14" y="4" width="6" height="6" rx="1" />
      <rect x="4" y="14" width="6" height="6" rx="1" />
      <rect x="14" y="14" width="6" height="6" rx="1" />
    </svg>
  );
}

/** Change Color — circle with inner fill. */
export function IconChangeColor(props: IconProps) {
  const { size, stroke, color } = defaults(props);
  return (
    <svg xmlns="http://www.w3.org/2000/svg" width={size} height={size} viewBox="0 0 24 24"
      fill="none" stroke={color} strokeWidth={stroke}
      strokeLinecap="round" strokeLinejoin="round" className={props.className}>
      <circle cx="12" cy="12" r="9" />
      <circle cx="12" cy="12" r="5" fill={color} fillOpacity="0.2" />
    </svg>
  );
}
