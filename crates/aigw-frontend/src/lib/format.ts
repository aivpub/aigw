// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Shared number formatting helpers
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/** Compact count/token formatting: 1.5B / 2.3M / 30K / 123.
 *  Whole units drop the trailing ".0" (1000000 → "1M", 30000 → "30K"). */
export function fmtTokens(v: number): string {
  const abs = Math.abs(v);
  if (abs >= 1_000_000_000)
    return `${trimTrailingZero((v / 1_000_000_000).toFixed(1))}B`;
  if (abs >= 1_000_000)
    return `${trimTrailingZero((v / 1_000_000).toFixed(1))}M`;
  if (abs >= 1_000) return `${trimTrailingZero((v / 1_000).toFixed(1))}K`;
  return v.toString();
}

/** Exact comma-separated value (e.g. "1,500,000,000") for tooltips / detail views. */
export function fmtExact(v: number): string {
  return Math.round(v).toLocaleString("en-US");
}

function trimTrailingZero(s: string): string {
  return s.endsWith(".0") ? s.slice(0, -2) : s;
}
