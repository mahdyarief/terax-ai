import { Button } from "@/components/ui/button";
import { writeClipboardText } from "@/lib/clipboard";
import { useEffect, useState } from "react";
import { toast } from "sonner";

type Props = {
  /** What is copied: the whole message or log, as plain text. */
  text: string;
  label?: string;
  variant?: "outline" | "ghost" | "secondary";
  size?: "xs" | "sm" | "default";
  className?: string;
};

/** Copies an error or a log, so it can be pasted into a report as is. */
export function CopyButton({
  text,
  label = "Copy",
  variant = "outline",
  size = "sm",
  className,
}: Props) {
  const [copied, setCopied] = useState(false);
  useEffect(() => {
    if (!copied) return;
    const timer = setTimeout(() => setCopied(false), 1500);
    return () => clearTimeout(timer);
  }, [copied]);
  return (
    <Button
      type="button"
      variant={variant}
      size={size}
      className={className}
      onClick={() => {
        writeClipboardText(text)
          .then(() => setCopied(true))
          .catch((e: unknown) =>
            toast.error("Couldn't copy", { description: String(e) }),
          );
      }}
    >
      {copied ? "Copied" : label}
    </Button>
  );
}
