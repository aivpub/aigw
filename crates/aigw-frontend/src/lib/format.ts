// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Shared number formatting helpers
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/** Compact count/token formatting: 1.50B / 2.30M / 30.00K / 123.
 *  Always two decimals above 1K (1000000 → "1.00M"); below 1K raw integer. */
export function fmtTokens(v: number): string {
  const abs = Math.abs(v);
  if (abs >= 1_000_000_000) return `${(v / 1_000_000_000).toFixed(2)}B`;
  if (abs >= 1_000_000) return `${(v / 1_000_000).toFixed(2)}M`;
  if (abs >= 1_000) return `${(v / 1_000).toFixed(2)}K`;
  return v.toString();
}

/** Exact comma-separated value (e.g. "1,500,000,000") for tooltips / detail views. */
export function fmtExact(v: number): string {
  return Math.round(v).toLocaleString("en-US");
}
