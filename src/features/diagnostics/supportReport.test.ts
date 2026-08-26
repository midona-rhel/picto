import { describe, expect, it } from 'vitest';
import { buildSupportReport, formatDiagnosticEntry, scrubSupportText } from './supportReport';

describe('support report', () => {
  it('keeps every log field as selectable tab-delimited text', () => {
    expect(formatDiagnosticEntry({
      id: 1,
      timestamp: '2026-08-26T12:00:00.000Z',
      level: 'WARN',
      source: 'ipc',
      target: 'items.query',
      durationMs: 20,
      nativeDurationMs: 3,
      message: 'A long message is not truncated',
    })).toBe('2026-08-26T12:00:00.000Z\tWARN\tipc\titems.query\t3.0 ms native / 20.0 ms total\tA long message is not truncated');
  });

  it('scrubs common personal paths and credentials', () => {
    expect(scrubSupportText('/Users/alice/Pictures test@example.com?token=secret Bearer abc.def'))
      .toBe('~/Pictures <email>?token=<redacted> Bearer <redacted>');
  });

  it('includes worker state and logs in one report', () => {
    const report = buildSupportReport([], [{
      id: 'thumb', label: 'Thumbnails', state: 'working', detail: 'one queued', active: 1, queued: 1, attention: 0,
    }], { userAgent: 'Picto/test', language: 'en' });
    expect(report).toContain('Workers\nID\tName\tState');
    expect(report).toContain('thumb\tThumbnails\tworking\t1\t1\t0\tone queued');
    expect(report).toContain('Logs\nTimestamp\tLevel\tSource');
  });
});
