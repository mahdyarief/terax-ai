import { cn } from "@/lib/utils";
import { useEffect, useState } from "react";

type Props = {
  src: string;
  /** File name for the alt text and tooltip. */
  name: string;
  /** Formatted file size, shown next to the pixel dimensions. */
  sizeLabel?: string;
  /** Keep the toolbar clear of the Rendered / Source switch drawn over it. */
  reserveToggleSpace?: boolean;
};

type Zoom = "fit" | "actual";

const CHECKERBOARD = {
  backgroundImage:
    "conic-gradient(var(--muted) 0.25turn, transparent 0.25turn 0.5turn, var(--muted) 0.5turn 0.75turn, transparent 0.75turn)",
  backgroundSize: "20px 20px",
} as const;

/** File name from a path with either separator (Windows paths use `\`). */
export function fileNameOf(path: string): string {
  return path.split(/[\\/]/).pop() ?? path;
}

/**
 * An image with its pixel size, a fit / 100% switch and a transparency grid
 * that can be turned off (a grid behind an opaque photo is only noise).
 */
export function ImagePreview({
  src,
  name,
  sizeLabel,
  reserveToggleSpace,
}: Props) {
  const [zoom, setZoom] = useState<Zoom>("fit");
  const [grid, setGrid] = useState(true);
  const [loaded, setLoaded] = useState<{
    src: string;
    width: number;
    height: number;
  } | null>(null);
  const [failedSrc, setFailedSrc] = useState<string | null>(null);

  // A different image starts fitted again.
  // biome-ignore lint/correctness/useExhaustiveDependencies: reset on a new file only.
  useEffect(() => setZoom("fit"), [name]);

  const dims = loaded?.src === src ? loaded : null;
  const failed = failedSrc === src;
  const meta = [
    dims && dims.width > 0 ? `${dims.width} × ${dims.height} px` : null,
    sizeLabel,
  ].filter(Boolean);

  return (
    <div className="flex h-full min-h-0 w-full flex-col bg-background">
      <div
        className={cn(
          "flex shrink-0 items-center justify-between gap-3 border-b border-border py-1.5 pl-3 text-xs text-muted-foreground",
          reserveToggleSpace ? "min-h-12 pr-44" : "pr-3",
        )}
      >
        <span className="min-w-0 truncate" title={name}>
          {meta.length > 0 ? meta.join(" · ") : name}
        </span>
        <div className="flex shrink-0 items-center gap-1">
          <ToolbarButton
            active={zoom === "fit"}
            onClick={() => setZoom("fit")}
            label="Fit"
            title="Fit the image to the pane"
          />
          <ToolbarButton
            active={zoom === "actual"}
            onClick={() => setZoom("actual")}
            label="100%"
            title="Show the image at its actual size"
          />
          <span className="mx-1 h-4 w-px bg-border" aria-hidden />
          <ToolbarButton
            active={grid}
            onClick={() => setGrid((value) => !value)}
            label="Grid"
            title="Show a grid behind transparent areas"
          />
        </div>
      </div>
      <div
        className={cn(
          "min-h-0 flex-1 overflow-auto p-4",
          zoom === "fit" && "flex items-center justify-center",
        )}
      >
        {failed ? (
          <p role="alert" className="text-xs text-muted-foreground">
            This image could not be displayed.
          </p>
        ) : (
          <img
            src={src}
            decoding="async"
            alt={name}
            onLoad={(event) =>
              setLoaded({
                src,
                width: event.currentTarget.naturalWidth,
                height: event.currentTarget.naturalHeight,
              })
            }
            onError={() => setFailedSrc(src)}
            className={cn(
              "rounded-md border border-border shadow-sm",
              zoom === "fit"
                ? "max-h-full max-w-full object-contain"
                : "max-w-none",
            )}
            style={grid ? CHECKERBOARD : undefined}
          />
        )}
      </div>
    </div>
  );
}

function ToolbarButton({
  active,
  onClick,
  label,
  title,
}: {
  active: boolean;
  onClick: () => void;
  label: string;
  title: string;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      title={title}
      aria-pressed={active}
      className={cn(
        "rounded px-2 py-0.5 transition-colors",
        active
          ? "bg-accent text-foreground"
          : "text-muted-foreground hover:text-foreground",
      )}
    >
      {label}
    </button>
  );
}
