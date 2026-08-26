interface GroupIconProps {
  size?: number;
  className?: string;
}

function IconFrame({
  size = 16,
  className,
  name,
  children,
}: GroupIconProps & { name: string; children: React.ReactNode }) {
  return (
    <svg
      aria-hidden="true"
      className={className}
      data-picto-icon={name}
      width={size}
      height={size}
      viewBox="0 0 20 20"
      fill="none"
    >
      {children}
    </svg>
  );
}

const strokeProps = {
  stroke: 'currentColor',
  strokeWidth: 1.25,
  strokeLinecap: 'round' as const,
  strokeLinejoin: 'round' as const,
};

const GROUP_FRONT = { x: 3, y: 1.5, width: 10.5, height: 14, radius: 1.5 } as const;

function GroupLayers() {
  return (
    <path
      d="M15.5 4v11.75a1.75 1.75 0 0 1-1.75 1.75H6.5"
      {...strokeProps}
    />
  );
}

export function GroupIcon(props: GroupIconProps) {
  return (
    <IconFrame {...props} name="group">
      <rect
        x={GROUP_FRONT.x}
        y={GROUP_FRONT.y}
        width={GROUP_FRONT.width}
        height={GROUP_FRONT.height}
        rx={GROUP_FRONT.radius}
        {...strokeProps}
      />
      <GroupLayers />
    </IconFrame>
  );
}

type GroupActionMark = 'create' | 'edit' | 'remove';

/**
 * Reserve the complete bottom-right quadrant for the action mark. The 1.25
 * unit clearance between these terminated strokes and every mark is the same
 * as the icon stroke itself, so composites stay legible at 15–16 CSS pixels.
 */
function GroupActionIcon({ mark }: { mark: GroupActionMark }) {
  return (
    <>
      <path
        d="M8.75 15.5H4.5A1.5 1.5 0 0 1 3 14V3a1.5 1.5 0 0 1 1.5-1.5H12A1.5 1.5 0 0 1 13.5 3v4.5"
        {...strokeProps}
      />
      <path d="M15.5 4v3.5M8.75 17.5H6.5" {...strokeProps} />
      <g data-picto-sub-icon={mark}>
        {mark === 'create' ? <path d="M11 14.75h7.5M14.75 11v7.5" {...strokeProps} /> : null}
        {mark === 'remove' ? <path d="m11.5 11.5 6.5 6.5m0-6.5-6.5 6.5" {...strokeProps} /> : null}
        {mark === 'edit' ? (
          <path d="m11.25 18.75 1.15-4.05 4.65-4.65 1.9 1.9-4.65 4.65-3.05 2.15Z" {...strokeProps} />
        ) : null}
      </g>
    </>
  );
}

export function drawGroupIcon(
  ctx: CanvasRenderingContext2D,
  x: number,
  y: number,
  size: number,
) {
  const scale = size / 20;
  ctx.save();
  ctx.translate(x, y);
  ctx.scale(scale, scale);
  ctx.lineWidth = 1.25;
  ctx.lineCap = 'round';
  ctx.lineJoin = 'round';
  ctx.beginPath();
  ctx.roundRect(
    GROUP_FRONT.x,
    GROUP_FRONT.y,
    GROUP_FRONT.width,
    GROUP_FRONT.height,
    GROUP_FRONT.radius,
  );
  ctx.moveTo(15.5, 4);
  ctx.lineTo(15.5, 15.75);
  ctx.arcTo(15.5, 17.5, 13.75, 17.5, 1.75);
  ctx.lineTo(6.5, 17.5);
  ctx.stroke();
  ctx.restore();
}

export function GroupCreateIcon(props: GroupIconProps) {
  return (
    <IconFrame {...props} name="group-create">
      <GroupActionIcon mark="create" />
    </IconFrame>
  );
}

export function GroupRemoveIcon(props: GroupIconProps) {
  return (
    <IconFrame {...props} name="group-remove">
      <GroupActionIcon mark="remove" />
    </IconFrame>
  );
}

export function GroupEditIcon(props: GroupIconProps) {
  return (
    <IconFrame {...props} name="group-edit">
      <GroupActionIcon mark="edit" />
    </IconFrame>
  );
}

function SelectionTiles() {
  return (
    <g opacity="0.72">
      <rect x="3" y="3" width="5.5" height="5.5" rx="1" {...strokeProps} />
      <rect x="11.5" y="3" width="5.5" height="5.5" rx="1" {...strokeProps} />
      <rect x="3" y="11.5" width="5.5" height="5.5" rx="1" {...strokeProps} />
      <rect x="11.5" y="11.5" width="5.5" height="5.5" rx="1" {...strokeProps} />
    </g>
  );
}

export function SelectAllIcon(props: GroupIconProps) {
  return (
    <IconFrame {...props} name="select-all">
      <SelectionTiles />
    </IconFrame>
  );
}

export function DeselectAllIcon(props: GroupIconProps) {
  return (
    <IconFrame {...props} name="deselect-all">
      <SelectionTiles />
      <path d="M2.5 2.5 17.5 17.5" {...strokeProps} />
    </IconFrame>
  );
}
