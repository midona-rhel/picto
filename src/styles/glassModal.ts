/** Mantine <Modal> styles object — glass panel effect. */
export const glassModalStyles = {
  content: {
    background: 'var(--box-background-glass)',
    backdropFilter: 'var(--box-background-blur)',
    WebkitBackdropFilter: 'var(--box-background-blur)',
    borderTop: 'var(--box-border-top)',
    borderRight: 'var(--box-border-right)',
    borderBottom: 'var(--box-border-bottom)',
    borderLeft: 'var(--box-border-left)',
    borderRadius: 'var(--box-border-radius)',
    boxShadow: 'var(--box-border-shadow-small)',
  },
  header: { background: 'transparent' },
  overlay: { backdropFilter: 'none', zIndex: 2001 },
  inner: { zIndex: 2001 },
} as const;
