import { cn } from "@/lib/utils";

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// ImageThumbnails — shared thumbnail strip for image data URLs
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
//
// Renders base64 data-URL images extracted from message/response content.
// Used by OutputCard, MessageBubble, InputCard and the Playground bubble.
// Only `data:image/` URLs are rendered (see extractImages in utils.ts).

interface ImageThumbnailsProps {
  images: string[];
  maxH?: string;
  maxW?: string;
}

export function ImageThumbnails({
  images,
  maxH = "h-32",
  maxW = "max-w-48",
}: ImageThumbnailsProps) {
  if (!images.length) return null;
  return (
    <div className="flex flex-wrap gap-2">
      {images.map((src, i) => (
        <img
          key={i}
          src={src}
          alt="image attachment"
          className={cn(
            `${maxH} ${maxW} rounded-md border object-contain`,
          )}
        />
      ))}
    </div>
  );
}
