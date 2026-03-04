# Enhanced Image Cache System

The Enhanced Image Cache system provides intelligent memory management and performance optimization for the ImageGrid component.

## Key Improvements

### 1. **Adaptive Memory Management**
- **Memory Pressure Detection**: Monitors JavaScript heap usage and adapts cache sizes automatically
- **Dynamic Cache Sizing**: Reduces cache limits under memory pressure (Normal → 70% → 40%)
- **Smart Eviction**: Uses LRU + access frequency scoring for intelligent cache eviction

### 2. **Enhanced Cache Performance**
- **Access Tracking**: Records access count and timestamps for better cache decisions
- **Batch Preloading**: Optimized concurrent loading with priority levels
- **Size Estimation**: Tracks actual blob sizes for accurate memory usage

### 3. **Memory Leak Prevention**
- **Automatic Cleanup**: Properly revokes ObjectURLs on eviction
- **Lifecycle Management**: Handles app shutdown, tab switching, and page unload
- **Desktop Runtime Integration**: Responds to app close events in Desktop Runtime environment

## Usage

### Basic Cache Operations

```typescript
import {
  getCachedMediaUrl,
  preloadMediaUrl,
  batchPreloadMediaUrls
} from './enhancedMediaCache';

// Get cached URL (if available)
const url = getCachedMediaUrl(imageHash, 'thumb512');

// Preload single image
await preloadMediaUrl(image, 'thumb512');

// Batch preload with priority
await batchPreloadMediaUrls(images, 'thumb64', 'high');
```

### Cache Monitoring

The system includes a visual cache monitor accessible via `Ctrl+C` in the ImageGrid:

- Real-time memory usage
- Cache hit ratios per variant
- Memory pressure indicators
- Automatic adaptation status

### Cache Statistics

```typescript
import { getCacheStats } from './enhancedMediaCache';

const stats = getCacheStats();
// Returns: { thumb64: { size, totalSize, memoryPressure }, ... }
```

## Cache Variants

| Variant | Size Limit | Est. Memory | Purpose |
|---------|------------|-------------|---------|
| `thumb64` | 10,000 | ~100MB | Fast scrolling thumbnails |
| `thumb512` | 1,000 | ~500MB | High-quality previews |
| `full` | 450 | ~1.8GB | Full resolution images |

## Memory Pressure Levels

- **Level 0 (Green)**: Normal operation, full cache capacity
- **Level 1 (Yellow)**: Medium pressure, 70% cache capacity
- **Level 2 (Red)**: High pressure, 40% cache capacity

## Performance Features

### Intelligent Preloading
- **Visible Range**: High-priority preloading for viewport images
- **Extended Range**: Low-priority preloading for scroll prediction
- **Neighbor Preloading**: Preloads adjacent images in detail view

### Request Deduplication
- Prevents duplicate network requests for the same image variant
- Shares promises across multiple requesters
- Eliminates cache race conditions

### Cleanup Automation
- Registers cleanup handlers on app initialization
- Handles browser tab visibility changes
- Integrates with Desktop Runtime app lifecycle events

## Configuration

Cache limits automatically adapt based on memory pressure. Base limits can be adjusted in `enhancedMediaCache.ts`:

```typescript
const BASE_MAX_ENTRIES: Record<MediaVariant, number> = {
  thumb64: 10000,   // Adjust for more/fewer small thumbnails
  thumb512: 1000,   // Adjust for more/fewer large thumbnails
  full: 450,        // Adjust for more/fewer full images
};
```

## Migration from Original Cache

The enhanced cache maintains API compatibility with the original `mediaCache.ts`:

- All existing `getCachedMediaUrl()` calls work unchanged
- All existing `preloadMediaUrl()` calls work unchanged
- Components automatically benefit from enhanced memory management

## Performance Monitoring

Enable the cache monitor with `Ctrl+C` in ImageGrid to see:
- Live cache usage and memory consumption
- Memory pressure adaptation in real-time
- Performance impact of different cache settings

## Debugging

For development, cache statistics are logged when memory pressure changes:

```
[MediaCache] Adapted to memory pressure level 1, new limits: {
  thumb64: 7000, thumb512: 700, full: 315
}
```

## Future Enhancements

Potential improvements for future versions:
- **IndexedDB Persistence**: Cross-session cache persistence
- **ML-based Preloading**: Predict user scroll patterns
- **WebWorker Integration**: Offload cache management to background threads
- **Compression**: Store compressed thumbnails for memory efficiency