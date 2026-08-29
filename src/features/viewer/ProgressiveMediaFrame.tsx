import type { CSSProperties, ReactNode, RefObject } from 'react';
import styles from './ProgressiveMediaFrame.module.css';

interface Props {
  frameRef?: RefObject<HTMLDivElement>;
  className?: string;
  style?: CSSProperties;
  dataAttributes?: Record<`data-${string}`, string>;
  preview: ReactNode;
  previewVisible: boolean;
  /** Reveal a retained preview atomically when final content is withdrawn. */
  instantPreviewWhenContentNotReady?: boolean;
  contentReady: boolean;
  children: ReactNode;
}

/** Keeps preview and final media in one geometry owner so readiness cannot reflow the viewer. */
export function ProgressiveMediaFrame({
  frameRef,
  className,
  style,
  dataAttributes,
  preview,
  previewVisible,
  instantPreviewWhenContentNotReady = false,
  contentReady,
  children,
}: Props) {
  return (
    <div
      ref={frameRef}
      className={`${styles.frame} ${className ?? ''}`}
      style={style}
      {...dataAttributes}
      data-progressive-media-frame
      data-ready={contentReady ? 'true' : 'false'}
      data-instant-preview={instantPreviewWhenContentNotReady ? 'true' : undefined}
    >
      <div
        className={`${styles.preview} ${previewVisible ? styles.previewVisible : ''}`}
        data-progressive-media-preview
        data-visible={previewVisible ? 'true' : 'false'}
      >
        {preview}
      </div>
      <div
        className={`${styles.content} ${contentReady ? styles.contentReady : ''}`}
        data-progressive-media-content
        data-visible={contentReady ? 'true' : 'false'}
      >
        {children}
      </div>
    </div>
  );
}
