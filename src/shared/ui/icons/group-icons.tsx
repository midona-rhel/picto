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

function GroupActionLayers() {
  return (
    <path
      d="M15.5 4v6.25M10.25 17.5H6.5"
      {...strokeProps}
    />
  );
}

function GroupCutout({ mark }: { mark: 'plus' | 'remove' }) {
  return (
    <>
      <path
        d="M10.25 15.5H4.5A1.5 1.5 0 0 1 3 14V3a1.5 1.5 0 0 1 1.5-1.5H12A1.5 1.5 0 0 1 13.5 3v7.25"
        {...strokeProps}
      />
      <GroupActionLayers />
      {mark === 'plus' ? (
        <path d="M12.5 13.5h5M15 11v5" {...strokeProps} />
      ) : (
        <path d="m13.25 11.75 3.5 3.5m0-3.5-3.5 3.5" {...strokeProps} />
      )}
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
      <GroupCutout mark="plus" />
    </IconFrame>
  );
}

export function GroupRemoveIcon(props: GroupIconProps) {
  return (
    <IconFrame {...props} name="group-remove">
      <GroupCutout mark="remove" />
    </IconFrame>
  );
}

export function GroupEditIcon(props: GroupIconProps) {
  return (
    <IconFrame {...props} name="group-edit">
      <path d="M10.25 15.5H4.5A1.5 1.5 0 0 1 3 14V3a1.5 1.5 0 0 1 1.5-1.5H12A1.5 1.5 0 0 1 13.5 3v6.25" {...strokeProps} />
      <GroupActionLayers />
      <path d="m10.75 15.25 1.15-2.3 3.6-3.6L17.15 11l-3.6 3.6-2.8.65Z" {...strokeProps} />
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
