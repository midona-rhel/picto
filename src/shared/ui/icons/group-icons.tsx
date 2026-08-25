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
  strokeWidth: 1.5,
  strokeLinecap: 'round' as const,
  strokeLinejoin: 'round' as const,
};

function GroupLayers() {
  return <path d="M4.5 15h11M5.5 18h9" {...strokeProps} />;
}

export function GroupIcon(props: GroupIconProps) {
  return (
    <IconFrame {...props} name="group">
      <rect x="3" y="2" width="14" height="10" rx="2" {...strokeProps} />
      <GroupLayers />
    </IconFrame>
  );
}

function GroupCutout({ mark }: { mark: 'plus' | 'remove' }) {
  return (
    <>
      <path
        d="M11.25 12H5a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h10a2 2 0 0 1 2 2v3.25"
        {...strokeProps}
      />
      <GroupLayers />
      {mark === 'plus' ? (
        <path d="M12.5 10.5h5M15 8v5" {...strokeProps} />
      ) : (
        <path d="m13.25 8.75 3.5 3.5m0-3.5-3.5 3.5" {...strokeProps} />
      )}
    </>
  );
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
      <path d="M11.5 11H5a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h10a2 2 0 0 1 2 2v2.5" {...strokeProps} />
      <GroupLayers />
      <path d="m11.5 11 1.15-2.3 3.6-3.6 1.65 1.65-3.6 3.6L12 11.5Z" {...strokeProps} />
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
