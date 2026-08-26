import { describe, expect, it } from 'vitest';
import {
  buildSupportReport,
  formatDiagnosticDisplayEntry,
  formatDiagnosticEntry,
  scrubSupportText,
} from './supportReport';

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

  it('aligns display columns without truncating long messages', () => {
    const base = {
      id: 1,
      timestamp: '2026-08-26T12:00:00.000Z',
      level: 'DEBUG' as const,
      source: 'ipc' as const,
      durationMs: 16.5,
      nativeDurationMs: 0.1,
    };
    const short = formatDiagnosticDisplayEntry({ ...base, target: 'settings.get', message: 'short message' }, '17:47:01.582');
    const long = formatDiagnosticDisplayEntry({ ...base, target: 'tags.namespace_counts', message: 'a complete long message' }, '17:47:01.582');

    expect(short.indexOf('0.1 ms native')).toBe(long.indexOf('0.1 ms native'));
    expect(short.indexOf('short message')).toBe(long.indexOf('a complete long message'));
    expect(long).toContain('a complete long message');
  });

  it('scrubs common personal paths and credentials', () => {
    const personalPath = ['', 'Users', 'example-user', 'Pictures'].join('/');
    expect(scrubSupportText(`${personalPath} test@example.com Bearer abc.def https://example.test/private?id=3`))
      .toBe('~/Pictures <email> Bearer <redacted> https://example.test/<redacted>');
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
