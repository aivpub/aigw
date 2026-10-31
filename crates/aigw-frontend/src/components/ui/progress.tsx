import { cn } from "@/lib/utils";

interface ProgressProps {
  /** Completed units. Clamped to `[0, max]` so a stale snapshot cannot overflow. */
  value: number;
  max: number;
  className?: string;
  "data-testid"?: string;
}

export function Progress({ value, max, className, ...rest }: ProgressProps) {
  const safeMax = max > 0 ? max : 1;
  const pct = Math.min(100, Math.max(0, (value / safeMax) * 100));
  return (
    <div
      role="progressbar"
      aria-valuenow={value}
      aria-valuemin={0}
      aria-valuemax={max}
      className={cn("h-2 w-full overflow-hidden rounded-full bg-muted", className)}
      {...rest}
    >
      <div
        className="h-full rounded-full bg-primary transition-[width] duration-300"
        style={{ width: `${pct}%` }}
      />
    </div>
  );
}
