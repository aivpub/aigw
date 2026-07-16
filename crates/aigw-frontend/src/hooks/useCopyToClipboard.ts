import { useState, useCallback, useRef } from "react";

/**
 * Copy text to clipboard.
 *
 * Simple approach — mirrors litellm's pattern:
 *   navigator.clipboard.writeText(text) wrapped in try/catch.
 *
 * Clipboard API works on localhost + HTTPS (both are "secure contexts").
 * No execCommand fallback — it silently drops content > ~32 KB and is
 * considered legacy.
 */
export function useCopyToClipboard(resetMs = 2000) {
  const [copied, setCopied] = useState(false);
  const timerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  const markCopied = useCallback(() => {
    setCopied(true);
    if (timerRef.current) clearTimeout(timerRef.current);
    timerRef.current = setTimeout(() => setCopied(false), resetMs);
  }, [resetMs]);

  const copy = useCallback(
    (text: string) => {
      if (!text) return;
      try {
        navigator.clipboard.writeText(text).then(
          () => markCopied(),
          () => {},
        );
      } catch {
        // clipboard API not available
      }
    },
    [markCopied],
  );

  return { copied, copy };
}
