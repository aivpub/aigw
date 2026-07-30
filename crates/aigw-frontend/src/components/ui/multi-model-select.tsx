import { useState, useRef, useEffect, useMemo } from "react";
import { useQuery } from "@tanstack/react-query";
import { apiGet } from "@/lib/api";
import { cn } from "@/lib/utils";
import { Badge } from "@/components/ui/badge";
import { Input } from "@/components/ui/input";
import { X, ChevronsUpDown } from "lucide-react";

interface ModelItem {
  model_name: string;
  model_id?: string;
}

interface ModelListResponse {
  data?: ModelItem[];
  total_count?: number;
}

interface MultiModelSelectProps {
  selected: string[];
  onChange: (models: string[]) => void;
  className?: string;
}

export function MultiModelSelect({
  selected,
  onChange,
  className,
}: MultiModelSelectProps) {
  const [open, setOpen] = useState(false);
  const [search, setSearch] = useState("");
  const inputRef = useRef<HTMLInputElement>(null);
  const containerRef = useRef<HTMLDivElement>(null);

  const { data } = useQuery<ModelListResponse>({
    queryKey: ["proxy-models", "all"],
    queryFn: () => apiGet("/model/list?page=1&page_size=200"),
    staleTime: 60_000,
  });

  const allModels = data?.data ?? [];

  const filteredModels = useMemo(() => {
    if (!search.trim()) return allModels;
    const q = search.toLowerCase();
    return allModels.filter(
      (m) =>
        m.model_name.toLowerCase().includes(q) ||
        (m.model_id ?? "").toLowerCase().includes(q),
    );
  }, [allModels, search]);

  const unselectedModels = useMemo(
    () => filteredModels.filter((m) => !selected.includes(m.model_name)),
    [filteredModels, selected],
  );

  // Close on outside click
  useEffect(() => {
    function handleClickOutside(e: MouseEvent) {
      if (containerRef.current && !containerRef.current.contains(e.target as Node)) {
        setOpen(false);
        setSearch("");
      }
    }
    if (open) {
      document.addEventListener("mousedown", handleClickOutside);
      return () => document.removeEventListener("mousedown", handleClickOutside);
    }
  }, [open]);

  function addModel(name: string) {
    onChange([...selected, name]);
    setSearch("");
    inputRef.current?.focus();
  }

  function removeModel(name: string) {
    onChange(selected.filter((m) => m !== name));
  }

  function handleKeyDown(e: React.KeyboardEvent) {
    if (e.key === "Backspace" && !search && selected.length > 0) {
      removeModel(selected[selected.length - 1]);
    }
    if (e.key === "Escape") {
      setOpen(false);
      setSearch("");
    }
  }

  return (
    <div ref={containerRef} className={cn("relative", className)}>
      {/* Chips + input row */}
      <div
        className={cn(
          "flex flex-wrap items-center gap-1 rounded-md border border-input bg-background px-3 py-2 min-h-[40px]",
          "focus-within:ring-1 focus-within:ring-ring",
        )}
        onClick={() => {
          setOpen(true);
          inputRef.current?.focus();
        }}
      >
        {selected.map((model) => (
          <Badge
            key={model}
            variant="secondary"
            className="gap-1 pr-1 cursor-default"
          >
            {model}
            <button
              type="button"
              onClick={(e) => {
                e.stopPropagation();
                removeModel(model);
              }}
              className="ml-0.5 rounded-full p-0.5 hover:bg-muted-foreground/20 transition-colors"
              aria-label={`Remove ${model}`}
            >
              <X className="h-3 w-3" />
            </button>
          </Badge>
        ))}
        <input
          ref={inputRef}
          type="text"
          value={search}
          onChange={(e) => {
            setSearch(e.target.value);
            if (!open) setOpen(true);
          }}
          onFocus={() => setOpen(true)}
          onKeyDown={handleKeyDown}
          placeholder={selected.length === 0 ? "Search models..." : ""}
          className="flex-1 min-w-[120px] bg-transparent border-0 outline-none text-sm placeholder:text-muted-foreground p-0"
        />
        <ChevronsUpDown className="h-4 w-4 shrink-0 text-muted-foreground ml-auto" />
      </div>

      {/* Dropdown */}
      {open && (
        <div className="absolute z-50 mt-1 w-full rounded-md border bg-popover text-popover-foreground shadow-md">
          <div className="max-h-48 overflow-y-auto p-1">
            {allModels.length > 0 && (
              <button
                type="button"
                onClick={() => {
                  const allModelNames = filteredModels.map((m) => m.model_name);
                  const addSet = new Set([...selected, ...allModelNames]);
                  onChange(Array.from(addSet));
                  setSearch("");
                  inputRef.current?.focus();
                }}
                className={cn(
                  "flex w-full items-center rounded-sm px-2 py-1.5 text-xs font-medium outline-none",
                  "hover:bg-accent hover:text-accent-foreground text-primary/80",
                  "border-b border-border mb-1 pb-2",
                )}
              >
                <span className="flex-1 text-left">Select All</span>
              </button>
            )}
            {unselectedModels.length === 0 ? (
              <div className="px-2 py-4 text-sm text-center text-muted-foreground">
                {allModels.length === 0
                  ? "Loading models..."
                  : "No matching models"}
              </div>
            ) : (
              unselectedModels.map((model) => (
                <button
                  key={model.model_name}
                  type="button"
                  onClick={() => addModel(model.model_name)}
                  className={cn(
                    "flex w-full items-center rounded-sm px-2 py-1.5 text-sm outline-none",
                    "hover:bg-accent hover:text-accent-foreground",
                    "focus:bg-accent focus:text-accent-foreground",
                  )}
                >
                  <span className="flex-1 text-left">{model.model_name}</span>
                </button>
              ))
            )}
          </div>
        </div>
      )}
    </div>
  );
}
