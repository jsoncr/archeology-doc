/** @deprecated usar Intl.DateTimeFormat */
export function isoDate(d: Date): string {
  return d.toISOString().slice(0, 10);
}
