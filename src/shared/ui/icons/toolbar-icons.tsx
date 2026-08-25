interface ToolbarIconProps {
  size?: number;
  className?: string;
}

function Svg({ size = 16, className, children }: ToolbarIconProps & { children: React.ReactNode }) {
  return (
    <svg aria-hidden="true" data-toolbar-glyph className={className} width={size} height={size} viewBox="0 0 16 16" fill="none">
      {children}
    </svg>
  );
}

/** Original Picto glyphs use filled one-pixel details for clean toolbar rasterization. */
export function ToolbarFilterIcon(props: ToolbarIconProps) {
  return (
    <Svg {...props}>
      <path fill="currentColor" fillRule="evenodd" d="M3.25 3h9.5a1 1 0 0 1 .7 1.71L9.5 8.66V12a1 1 0 0 1-1.45.9l-1.5-.75A1 1 0 0 1 6 11.25V8.66L2.55 4.7A1 1 0 0 1 3.25 3Zm.7 1L7 7.95v3.3l1.5.75V7.95L12.05 4h-8.1Z" />
    </Svg>
  );
}

export function ToolbarLayoutIcon(props: ToolbarIconProps) {
  return (
    <Svg {...props}>
      <rect x="2.5" y="2.5" width="11" height="11" rx="2" stroke="currentColor" />
      <path fill="currentColor" d="M6 3h1v11H6zM3 6h11v1H3z" />
    </Svg>
  );
}

export function ToolbarPanelIcon(props: ToolbarIconProps) {
  return (
    <Svg {...props}>
      <rect x="1.5" y="1.5" width="13" height="13" rx="2.5" stroke="currentColor" />
      <path fill="currentColor" d="M5 2h1v12H5z" />
    </Svg>
  );
}

export function ToolbarChevronIcon({ direction, ...props }: ToolbarIconProps & { direction: 'left' | 'right' }) {
  return (
    <Svg {...props}>
      <path fill="currentColor" d={direction === 'left' ? 'M10.1 3.15a.5.5 0 0 1 .7.7L6.65 8l4.15 4.15a.5.5 0 0 1-.7.7l-4.5-4.5a.5.5 0 0 1 0-.7l4.5-4.5Z' : 'M5.9 3.15a.5.5 0 0 0-.7.7L9.35 8 5.2 12.15a.5.5 0 0 0 .7.7l4.5-4.5a.5.5 0 0 0 0-.7l-4.5-4.5Z'} />
    </Svg>
  );
}

/** Browser/history navigation — intentionally distinct from media chevrons. */
export function ToolbarHistoryIcon({ direction, ...props }: ToolbarIconProps & { direction: 'back' | 'forward' }) {
  const back = direction === 'back';
  return (
    <Svg {...props}>
      <path
        d={back ? 'M6.5 3.5 2 8l4.5 4.5M2 8h12' : 'm9.5 3.5 4.5 4.5-4.5 4.5M14 8H2'}
        stroke="currentColor"
        strokeWidth="1"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
    </Svg>
  );
}

/** Inspector label controls use a 15px pixel-snapped filled plus. */
export function InspectorAddIcon({ className }: Pick<ToolbarIconProps, 'className'>) {
  return (
    <svg aria-hidden="true" className={className} width="15" height="15" viewBox="0 0 15 15" fill="none">
      <path fill="currentColor" d="M7 2h1v5h5v1H8v5H7V8H2V7h5V2Z" />
    </svg>
  );
}

/** Inspector export uses filled one-pixel geometry so it stays sharp at 1×. */
export function InspectorExportIcon({ className }: Pick<ToolbarIconProps, 'className'>) {
  return (
    <svg aria-hidden="true" className={className} width="16" height="16" viewBox="0 0 16 16" fill="none">
      <path fill="currentColor" d="M7 2.7 3.85 5.85l-.7-.7 4-4a.5.5 0 0 1 .7 0l4 4-.7.7L8 2.7V11H7V2.7Z" />
      <path fill="currentColor" d="M1 10h1v3a1 1 0 0 0 1 1h9a1 1 0 0 0 1-1v-3h1v3a2 2 0 0 1-2 2H3a2 2 0 0 1-2-2v-3Z" />
    </svg>
  );
}

/** Inspector chip remove control uses an 18px box and a crisp one-pixel X. */
export function InspectorRemoveIcon({ className }: Pick<ToolbarIconProps, 'className'>) {
  return (
    <svg aria-hidden="true" className={className} width="18" height="18" viewBox="0 0 18 18" fill="none">
      <path d="m5.5 5.5 7 7m0-7-7 7" stroke="currentColor" strokeWidth="1" strokeLinecap="round" />
    </svg>
  );
}

export function ToolbarCloseIcon(props: ToolbarIconProps) {
  return (
    <Svg {...props}>
      <path stroke="currentColor" strokeWidth="1" strokeLinecap="round" d="m4 4 8 8m0-8-8 8" />
    </Svg>
  );
}

export function ToolbarMinusIcon(props: ToolbarIconProps) {
  return <Svg {...props}><path fill="currentColor" d="M3 7.5h10v1H3z" /></Svg>;
}

export function ToolbarPlusIcon(props: ToolbarIconProps) {
  return <Svg {...props}><path fill="currentColor" d="M3 7.5h10v1H3zM7.5 3h1v10h-1z" /></Svg>;
}

export function ToolbarActualSizeIcon(props: ToolbarIconProps) {
  return (
    <Svg {...props}>
      <path fill="currentColor" d="M2 2h5v1H3v4H2V2Zm12 0v5h-1V3H9V2h5ZM2 14V9h1v4h4v1H2Zm12 0H9v-1h4V9h1v5Z" />
    </Svg>
  );
}

export function ToolbarFitIcon(props: ToolbarIconProps) {
  return (
    <Svg {...props}>
      <path fill="currentColor" d="M2 6V2h4v1H3v3H2Zm8-4h4v4h-1V3h-3V2ZM2 10h1v3h3v1H2v-4Zm11 0h1v4h-4v-1h3v-3Z" />
      <rect x="5.5" y="5.5" width="5" height="5" rx=".5" stroke="currentColor" />
    </Svg>
  );
}
