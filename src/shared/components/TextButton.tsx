import type { ButtonHTMLAttributes, ReactNode } from 'react';

interface TextButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  compact?: boolean;
  danger?: boolean;
  children: ReactNode;
}

export function TextButton({ compact, danger, className, children, ...rest }: TextButtonProps) {
  const cls = [
    'textBtn',
    compact && 'textBtnCompact',
    danger && 'textBtnDanger',
    className,
  ].filter(Boolean).join(' ');

  return <button className={cls} {...rest}>{children}</button>;
}
