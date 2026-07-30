import { Button } from "@/components/ui/button";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { ChevronLeft, ChevronRight } from "lucide-react";

export interface PaginationBarProps {
  page: number;
  pageSize: number;
  totalCount: number;
  totalPages: number;
  onPage: (p: number) => void;
  onPageSize: (s: number) => void;
}

/**
 * Shared server-side pagination control.
 *
 * Renders the showing range and page info on the left, and a page-size
 * selector + prev/next buttons on the right. Used above and below paged
 * tables; the parent decides when to show it (e.g. hidden when the
 * current page is empty).
 *
 * Behavior matches the original PaginationBar in pages/spend-logs/index.tsx.
 */
export function PaginationBar({
  page,
  pageSize,
  totalCount,
  totalPages,
  onPage,
  onPageSize,
}: PaginationBarProps) {
  const from = totalCount === 0 ? 0 : (page - 1) * pageSize + 1;
  const to = Math.min(page * pageSize, totalCount);
  return (
    <div className="flex flex-col sm:flex-row items-start sm:items-center justify-between gap-2">
      <div className="flex items-center gap-3">
        <span className="text-xs text-muted-foreground">
          Showing {from}–{to} of {totalCount}
        </span>
        <span className="text-xs text-muted-foreground">
          Page {page} of {Math.max(totalPages, 1)}
        </span>
      </div>
      <div className="flex items-center gap-2">
        <Select value={String(pageSize)} onValueChange={(v) => onPageSize(Number(v))}>
          <SelectTrigger className="h-7 w-[70px] text-xs">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="30">30</SelectItem>
            <SelectItem value="50">50</SelectItem>
            <SelectItem value="100">100</SelectItem>
          </SelectContent>
        </Select>
        <Button
          variant="outline"
          size="sm"
          disabled={page <= 1}
          onClick={() => onPage(page - 1)}
          className="h-7 px-2"
        >
          <ChevronLeft className="h-3.5 w-3.5" />
        </Button>
        <Button
          variant="outline"
          size="sm"
          disabled={page >= totalPages || totalPages === 0}
          onClick={() => onPage(page + 1)}
          className="h-7 px-2"
        >
          <ChevronRight className="h-3.5 w-3.5" />
        </Button>
      </div>
    </div>
  );
}
