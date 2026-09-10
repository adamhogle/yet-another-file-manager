import { describe, expect, it } from 'vitest';

import { formatModified, formatSize } from '../frontend/src/lib/format';

const referenceFormatter = new Intl.DateTimeFormat(undefined, {
  year: 'numeric',
  month: '2-digit',
  day: '2-digit',
  hour: '2-digit',
  minute: '2-digit'
});

describe('formatSize', () => {
  it('formats zero and sub-kilobyte byte counts', () => {
    expect(formatSize(0)).toBe('0 B');
    expect(formatSize(999)).toBe('999 B');
    expect(formatSize(1023)).toBe('1023 B');
  });

  it('formats exact 1024-based unit boundaries', () => {
    expect(formatSize(1024)).toBe('1.00 KB');
    expect(formatSize(1024 ** 2)).toBe('1.00 MB');
    expect(formatSize(1024 ** 3)).toBe('1.00 GB');
    expect(formatSize(1024 ** 4)).toBe('1.00 TB');
  });

  it('switches precision at value boundaries and caps at TB', () => {
    expect(formatSize(10240)).toBe('10.0 KB');
    expect(formatSize(102400)).toBe('100 KB');
    expect(formatSize(1024 ** 5)).toBe('1024 TB');
  });

  it('rounds fractional values to the active precision', () => {
    expect(formatSize(1536)).toBe('1.50 KB');
    expect(formatSize(2053.12)).toBe('2.00 KB');
  });

  it('passes negative input through in the byte range and falls back for null/undefined', () => {
    expect(formatSize(-5)).toBe('-5 B');
    expect(formatSize(undefined)).toBe('NaN KB');
    expect(formatSize(null)).toBe('');
  });
});

describe('formatModified', () => {
  it('formats a valid ISO string the same way the component formatter does', () => {
    const iso = '2026-01-05T03:04:00Z';
    const expected = referenceFormatter.format(new Date(iso));

    expect(formatModified(iso)).toBe(expected);
  });

  it('renders empty for absent, empty, and malformed input', () => {
    expect(formatModified(null)).toBe('');
    expect(formatModified(undefined)).toBe('');
    expect(formatModified('')).toBe('');
    expect(formatModified('not-a-date')).toBe('');
  });
});
