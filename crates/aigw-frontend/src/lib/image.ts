/**
 * Client-side image helpers.
 *
 * TD-009a: Playground image compression before upload. Large photos (multi-MB
 * JPEG/PNG) inflate the base64 request body and token cost; we downscale the
 * longest edge to `MAX_EDGE` px and re-encode at JPEG quality 0.8 when the
 * result is actually smaller than the original.
 */

/** Longest edge (px) for the compressed output. Stage 114 design value. */
export const MAX_IMAGE_EDGE = 2048;

/** JPEG quality for lossy re-encode. */
export const IMAGE_JPEG_QUALITY = 0.8;

/**
 * Estimate the total byte size of a base64 data URL (the "data:image/...;base64," part).
 * Used for the pre-send body-limit defense (TD-009b): `∑ data URL lengths` is
 * compared against the gateway body limit.
 */
export function dataUrlBytes(dataUrl: string): number {
  const comma = dataUrl.indexOf(",");
  if (comma === -1) return dataUrl.length;
  const b64 = dataUrl.slice(comma + 1);
  // base64 → bytes: 4 chars encode 3 bytes (ignore padding).
  return Math.floor((b64.length * 3) / 4);
}

export interface CompressResult {
  /** The compressed data URL, or null when compression was skipped (too small / decode failure). */
  dataUrl: string | null;
  /** Original file size in bytes (for the "compressed < original" assertion). */
  originalBytes: number;
  /** Compressed data-URL byte size (0 when skipped). */
  compressedBytes: number;
}

/**
 * Downscale + re-encode an image File to a JPEG data URL.
 *
 * - Decodes via `createImageBitmap` (or `Image` fallback).
 * - Scales the longest edge down to `MAX_IMAGE_EDGE`, preserving aspect ratio.
 * - Encodes to `image/jpeg` at `IMAGE_JPEG_QUALITY`; returns the ORIGINAL data
 *   URL when the re-encode is not smaller (lossless PNG photos may not shrink).
 * - Returns null when the browser cannot decode the image (e.g. HEIC outside
 *   Safari — the TD-011b frontend-transcode note).
 */
export async function compressImage(file: File): Promise<CompressResult> {
  const originalBytes = file.size;
  const originalDataUrl = await fileToDataUrl(file);
  if (!originalDataUrl) return { dataUrl: null, originalBytes, compressedBytes: 0 };

  let bitmap: ImageBitmap | HTMLImageElement;
  try {
    bitmap = await loadBitmap(file);
  } catch {
    // Undecodable by this browser (HEIC/AVIF on Chromium/Firefox). Keep the
    // original — the upload path already guards RASTER_MIME, and this file
    // would have been rejected upstream anyway.
    return { dataUrl: originalDataUrl, originalBytes, compressedBytes: dataUrlBytes(originalDataUrl) };
  }

  const { width, height } = bitmapSize(bitmap);
  if (width === 0 || height === 0 || (width <= MAX_IMAGE_EDGE && height <= MAX_IMAGE_EDGE)) {
    // Small image — no downscale needed; return original (small images pass
    // through untouched so tiny avatars stay crisp).
    closeBitmap(bitmap);
    return { dataUrl: originalDataUrl, originalBytes, compressedBytes: dataUrlBytes(originalDataUrl) };
  }

  // Scale longest edge → MAX_IMAGE_EDGE, keep aspect ratio.
  const scale = Math.min(1, MAX_IMAGE_EDGE / Math.max(width, height));
  const outW = Math.max(1, Math.round(width * scale));
  const outH = Math.max(1, Math.round(height * scale));

  const canvas = document.createElement("canvas");
  canvas.width = outW;
  canvas.height = outH;
  const ctx = canvas.getContext("2d");
  if (!ctx) {
    closeBitmap(bitmap);
    return { dataUrl: originalDataUrl, originalBytes, compressedBytes: dataUrlBytes(originalDataUrl) };
  }
  ctx.drawImage(bitmap, 0, 0, outW, outH);
  closeBitmap(bitmap);

  // JPEG 0.8 — drop alpha (fills with white first so transparent PNGs don't
  // turn black; a plain JPEG has no alpha channel).
  ctx.fillStyle = "#ffffff";
  ctx.fillRect(0, 0, outW, outH);
  ctx.drawImage(canvas, 0, 0); // re-draw after white fill
  const jpeg = canvas.toDataURL("image/jpeg", IMAGE_JPEG_QUALITY);

  const compressedBytes = dataUrlBytes(jpeg);
  if (compressedBytes >= dataUrlBytes(originalDataUrl)) {
    // Re-encode not smaller — keep original (preserves PNG fidelity + alpha).
    return { dataUrl: originalDataUrl, originalBytes, compressedBytes };
  }
  return { dataUrl: jpeg, originalBytes, compressedBytes };
}

/** Read a File to a base64 data URL. */
export function fileToDataUrl(file: File): Promise<string | null> {
  return new Promise((resolve) => {
    const reader = new FileReader();
    reader.onload = () => resolve(typeof reader.result === "string" ? reader.result : null);
    reader.onerror = () => resolve(null);
    reader.readAsDataURL(file);
  });
}

async function loadBitmap(file: File): Promise<ImageBitmap | HTMLImageElement> {
  if (typeof createImageBitmap === "function") {
    try {
      return await createImageBitmap(file);
    } catch {
      // fall through to <img> decode
    }
  }
  return new Promise<HTMLImageElement>((resolve, reject) => {
    const url = URL.createObjectURL(file);
    const img = new Image();
    img.onload = () => {
      URL.revokeObjectURL(url);
      resolve(img);
    };
    img.onerror = () => {
      URL.revokeObjectURL(url);
      reject(new Error("image decode failed"));
    };
    img.src = url;
  });
}

function bitmapSize(bmp: ImageBitmap | HTMLImageElement): { width: number; height: number } {
  return { width: bmp.width, height: bmp.height };
}

function closeBitmap(bmp: ImageBitmap | HTMLImageElement): void {
  if (typeof createImageBitmap === "function" && "close" in bmp && typeof bmp.close === "function") {
    bmp.close();
  }
}
