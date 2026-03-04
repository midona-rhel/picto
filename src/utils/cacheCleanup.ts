import { cleanupMediaCache } from '../components/image-grid/enhancedMediaCache';

// Register cleanup handlers for the enhanced media cache
export function registerCacheCleanup(): void {
  if (typeof window !== 'undefined') {
    // Clean up on page unload
    window.addEventListener('beforeunload', cleanupMediaCache);

    // Clean up on visibility change (when tab becomes hidden)
    document.addEventListener('visibilitychange', () => {
      if (document.visibilityState === 'hidden') {
        // Optionally trigger garbage collection if available
        if ('gc' in window) {
          setTimeout(() => {
            try {
              (window as any).gc();
            } catch {
              // Ignore if gc is not available
            }
          }, 100);
        }
      }
    });

    // App termination is handled by beforeunload in Electron.
  }
}

// Optional: Force cleanup (can be called manually)
export function forceCacheCleanup(): void {
  cleanupMediaCache();

  // Trigger garbage collection if available
  if (typeof window !== 'undefined' && 'gc' in window) {
    try {
      (window as any).gc();
    } catch {
      // Ignore if gc is not available
    }
  }
}
