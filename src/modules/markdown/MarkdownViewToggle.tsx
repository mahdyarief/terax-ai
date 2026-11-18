import { cn } from "@/lib/utils";

export type MarkdownViewMode = "rendered" | "split" | "raw";

type Props = {
  mode: MarkdownViewMode;
  onChange: (mode: MarkdownViewMode) => void;
  /** Offer the side-by-side view (source and preview together). */
  split?: boolean;
  renderedDisabled?: boolean;
  renderedHint?: string;
};

const LABELS: Record<MarkdownViewMode, string> = {
  rendered: "Rendered",
  split: "Split",
  raw: "Raw",
};

export function MarkdownViewToggle({
  mode,
  onChange,
  split = false,
  renderedDisabled,
  renderedHint,
}: Props) {
  const modes: MarkdownViewMode[] = split
    ? ["rendered", "split", "raw"]
    : ["rendered", "raw"];
  return (
    <div className="absolute right-3 top-3 z-10 inline-flex items-center gap-0.5 rounded-md border border-border/60 bg-card p-0.5 text-[11px] shadow-sm">
      {modes.map((value) => {
        const disabled = value === "rendered" && renderedDisabled;
        return (
          <button
            key={value}
            type="button"
            onClick={() => onChange(value)}
            disabled={disabled}
            aria-pressed={mode === value}
            title={disabled ? renderedHint : undefined}
            className={cn(
              "rounded px-2 py-0.5 transition-colors",
              mode === value
                ? "bg-accent text-foreground"
                : "text-muted-foreground hover:text-foreground",
              disabled &&
                "cursor-not-allowed opacity-40 hover:text-muted-foreground",
            )}
          >
            {LABELS[value]}
          </button>
        );
      })}
    </div>
  );
}
