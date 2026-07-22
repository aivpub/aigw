import { Button } from "@/components/ui/button";
import { Copy, Check } from "lucide-react";
import { useCopyToClipboard } from "@/hooks/useCopyToClipboard";
import { toast } from "sonner";

interface CopyButtonProps {
  text: string;
  label?: string;
  className?: string;
}

export function CopyButton({ text, label, className = "" }: CopyButtonProps) {
  const { copied, copy } = useCopyToClipboard({
    onError: () => toast.error("Copy failed — clipboard unavailable"),
  });

  return (
    <Button
      variant="ghost"
      size="sm"
      className={`h-7 text-xs gap-1 ${className}`}
      onClick={() => copy(text)}
    >
      {copied ? (
        <Check className="h-3 w-3 text-green-500" />
      ) : (
        <Copy className="h-3 w-3" />
      )}
      {label && <span>{label}</span>}
    </Button>
  );
}
