import { useTranslation } from "react-i18next";
import { Badge } from "@/components/ui/badge";
import { ExternalLink } from "lucide-react";

/// Projected server-side by `aigw-server/src/routes/search_view.rs`. The raw
/// `response` blob stays on the row for the "raw" tab; this is the bounded
/// view (≤20 results, ≤300-char snippets, http(s)-only URLs).
export interface SearchResultItem {
  title: string;
  url: string | null;
  domain: string | null;
  snippet: string | null;
  score: number | null;
  published_date: string | null;
}

export interface SearchView {
  provider: string | null;
  instance: string | null;
  query: string | null;
  query_count: number | null;
  result_count: number | null;
  kept_count: number;
  cost_per_query: number | null;
  parent_call_id: string | null;
  results: SearchResultItem[];
}

/// The drawer's "visual" tab for a search row.
///
/// Deliberately not `OutputCard`: that component's `parseOutput` recognises
/// chat `choices[]` / responses `output[]` / embeddings `data[]` and falls
/// through to blank for this shape. Search renders here instead, so the
/// generic component stays untouched for the three surfaces that do share it.
export function SearchResultsPanel({ view }: { view: SearchView }) {
  const { t } = useTranslation();
  const { results, result_count: total, kept_count: kept } = view;

  const heading =
    total != null && total !== kept
      ? t("spendLogs.search.keptOfTotal", { kept, total })
      : `(${kept})`;

  return (
    <div className="space-y-2" data-testid="search-results-panel">
      <div className="flex items-center gap-2 text-xs">
        <span className="font-medium">
          {t("spendLogs.search.resultsTitle")}
        </span>
        <span className="text-muted-foreground">{heading}</span>
      </div>

      {results.length === 0 ? (
        <p className="text-xs text-muted-foreground italic py-2">
          {total != null && total > 0
            ? t("spendLogs.search.noResultsKept")
            : t("spendLogs.drawer.noResponseData")}
        </p>
      ) : (
        <div className="rounded-md border divide-y">
          {results.map((r, i) => (
            <ResultCard key={`${r.url ?? r.title}-${i}`} result={r} />
          ))}
        </div>
      )}
    </div>
  );
}

function ResultCard({ result }: { result: SearchResultItem }) {
  const { t } = useTranslation();
  return (
    <div className="p-2.5 space-y-0.5" data-testid="search-result-item">
      <div className="flex items-start justify-between gap-2">
        <div className="text-xs font-medium leading-snug min-w-0">
          {result.url ? (
            <a
              href={result.url}
              target="_blank"
              rel="noopener noreferrer"
              className="hover:underline inline-flex items-center gap-1 break-all"
              title={t("spendLogs.search.openResult")}
              data-testid="search-result-title"
            >
              {result.title}
              <ExternalLink className="h-3 w-3 shrink-0" />
            </a>
          ) : (
            <span className="break-all" data-testid="search-result-title">
              {result.title}
            </span>
          )}
        </div>
        {result.score != null ? (
          <Badge
            variant="outline"
            className="text-[10px] px-1 py-0 shrink-0 font-mono"
          >
            {result.score.toFixed(1)}
          </Badge>
        ) : null}
      </div>

      <div className="flex items-center gap-2 text-[10px] text-muted-foreground">
        {result.domain ? <span className="truncate">{result.domain}</span> : null}
        {result.published_date ? (
          <span className="truncate">{result.published_date}</span>
        ) : null}
      </div>

      {result.snippet ? (
        <p className="text-[11px] text-muted-foreground leading-snug line-clamp-3">
          {result.snippet}
        </p>
      ) : null}
    </div>
  );
}
