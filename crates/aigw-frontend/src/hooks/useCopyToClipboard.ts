import { useState, useCallback } from "react";

export function useCopyToClipboard(resetMs = 2000) {
  const [copied, setCopied] = useState(false);

  const copy = useCallback(async (text: string) => {
    try {
      // Prefer the async Clipboard API (requires secure context)
      await navigator.clipboard.writeText(text);
      setCopied(true);
      setTimeout(() => setCopied(false), resetMs);
    } catch {
      // Fallback for non-HTTPS / older browsers: execCommand
      try {
        const ta = document.createElement("textarea");
        ta.value = text;
        ta.style.position = "fixed";
        ta.style.left = "-9999px";
        ta.style.top = "-9999px";
        document.body.appendChild(ta);
        ta.focus();
        ta.select();
        const success = document.execCommand("copy");
        document.body.removeChild(ta);
        if (success) {
          setCopied(true);
          setTimeout(() => setCopied(false), resetMs);
        }
      } catch {
        // both methods failed — silently ignore
      }
    }
  }, [resetMs]);

  return { copied, copy };
}
