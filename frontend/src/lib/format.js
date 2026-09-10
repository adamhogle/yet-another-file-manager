const dateFormatter = new Intl.DateTimeFormat(undefined, {
  year: 'numeric',
  month: '2-digit',
  day: '2-digit',
  hour: '2-digit',
  minute: '2-digit'
});

export function formatSize(sizeBytes) {
  if (sizeBytes === null) {
    return '';
  }

  if (sizeBytes < 1024) {
    return `${sizeBytes} B`;
  }

  const units = ['KB', 'MB', 'GB', 'TB'];
  let value = sizeBytes / 1024;
  let unitIndex = 0;

  while (value >= 1024 && unitIndex < units.length - 1) {
    value /= 1024;
    unitIndex += 1;
  }

  const precision = value >= 100 ? 0 : value >= 10 ? 1 : 2;
  return `${value.toFixed(precision)} ${units[unitIndex]}`;
}

// Malformed or absent input renders as empty rather than a fabricated timestamp:
// entries carry modifiedAt: null when the backend cannot stat the file, and
// new Date(null) would otherwise silently format as the epoch.
export function formatModified(isoString) {
  if (isoString === null || isoString === undefined) {
    return '';
  }

  const date = new Date(isoString);
  if (Number.isNaN(date.getTime())) {
    return '';
  }

  return dateFormatter.format(date);
}
