/** Time ranges shared by every page that filters by a window. */
export type TimePreset = "15m" | "1h" | "4h" | "24h" | "7d" | "30d" | "custom";

const PRESET_MS: Record<Exclude<TimePreset, "custom">, number> = {
  "15m": 15 * 60 * 1000,
  "1h": 3600 * 1000,
  "4h": 4 * 3600 * 1000,
  "24h": 24 * 3600 * 1000,
  "7d": 7 * 24 * 3600 * 1000,
  "30d": 30 * 24 * 3600 * 1000,
};

/**
 * `custom` starts from a 4h span rather than a zero-width one, so switching to
 * manual input does not blank the range the user was already looking at.
 */
export function presetRange(p: TimePreset) {
  const now = Date.now();
  const span = p === "custom" ? PRESET_MS["4h"] : PRESET_MS[p];
  return {
    start: new Date(now - span).toISOString(),
    end: new Date(now).toISOString(),
  };
}

/** ISO string → `datetime-local` value (YYYY-MM-DDTHH:MM, viewer's timezone). */
export function toDatetimeLocalValue(iso: string): string {
  if (!iso) return "";
  const d = new Date(iso);
  if (isNaN(d.getTime())) return "";
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}T${pad(
    d.getHours(),
  )}:${pad(d.getMinutes())}`;
}

export function fromDatetimeLocalValue(local: string): string {
  if (!local) return "";
  return new Date(local).toISOString();
}
