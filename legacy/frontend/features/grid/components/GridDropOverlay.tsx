export function GridDropOverlay() {
  return (
    <div
      style={{
        position: 'absolute',
        zIndex: 1002,
        top: 0,
        left: 0,
        right: 0,
        bottom: 0,
        boxSizing: 'border-box',
        border: '2px solid var(--color-primary)',
        backgroundColor: 'var(--color-primary-10, rgba(59, 130, 246, 0.1))',
        borderRadius: 8,
        cursor: 'copy',
        pointerEvents: 'none',
      }}
    >
      <div
        style={{
          position: 'absolute',
          bottom: 16,
          left: '50%',
          width: 200,
          marginLeft: -100,
          padding: 12,
          textAlign: 'center',
          color: 'var(--color-white-99)',
          fontSize: 'var(--font-size-md)',
          fontWeight: 'var(--font-weight-bold)',
          background: 'var(--color-primary)',
          lineHeight: 'var(--line-height-relaxed)',
          borderRadius: 6,
          pointerEvents: 'none',
          animation: 'pulse 0.8s infinite',
        }}
      >
        Drop files to import
      </div>
    </div>
  );
}
