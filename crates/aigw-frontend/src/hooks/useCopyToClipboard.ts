import { useState, useCallback, useRef } from "react";

export interface UseCopyOptions {
  /** Reset copied state after this many ms (default 2000). */
  resetMs?: number;
  /** Called when both clipboard API and execCommand fallback fail. */
  onError?: (text: string, err?: unknown) => void;
}

/**
 * Copy text to clipboard.
 *
 * Try navigator.clipboard first (secure contexts: localhost, HTTPS).
 * Fall back to execCommand for HTTP environments.
 */
export function useCopyToClipboard(opts: UseCopyOptions = {}) {
  const { resetMs = 2000, onError } = opts;
  const [copied, setCopied] = useState(false);
  const timerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  const markCopied = useCallback(() => {
    setCopied(true);
    if (timerRef.current) clearTimeout(timerRef.current);
    timerRef.current = setTimeout(() => setCopied(false), resetMs);
  }, [resetMs]);

  const copy = useCallback(
    (text: string) => {
      if (!text) {
        onError?.(text, new Error("empty text"));
        return;
      }
      try {
        if (typeof navigator.clipboard?.writeText === "function") {
          navigator.clipboard.writeText(text).then(
            () => markCopied(),
            (err) => fallbackCopy(text, err),
          );
        } else {
          fallbackCopy(text);
        }
      } catch (err) {
        fallbackCopy(text, err);
      }

      function fallbackCopy(t: string, originalErr?: unknown) {
        const textarea = document.createElement("textarea");
        textarea.value = t;
        textarea.style.position = "fixed";
        textarea.style.opacity = "0";
        document.body.appendChild(textarea);
        textarea.select();
        try {
          document.execCommand("copy");
          markCopied();
        } catch (execErr) {
          onError?.(t, execErr);
        }
        document.body.removeChild(textarea);
      }
    },
    [markCopied, onError],
  );

  return { copied, copy };
}
