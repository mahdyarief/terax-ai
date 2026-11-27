import { useEffect, useRef, useState } from "react";
import { PHONE_KEYS, type PhoneKey } from "./input";

const ROW: { key: PhoneKey | "ctrl"; label: string }[] = [
  { key: "escape", label: "Esc" },
  { key: "tab", label: "Tab" },
  { key: "ctrl", label: "Ctrl" },
  { key: "left", label: "←" },
  { key: "up", label: "↑" },
  { key: "down", label: "↓" },
  { key: "right", label: "→" },
  { key: "enter", label: "Enter" },
];

type Props = {
  onBytes: (bytes: Uint8Array) => void;
  onPasteText: (text: string) => void;
  ctrl: boolean;
  onCtrl: (next: boolean) => void;
};

/**
 * Keys a phone keyboard does not have, and a paste that goes as one write.
 * Every key is an Escape sequence from Ghostty's own table, so a shell in the
 * application cursor mode gets what it expects.
 */
export function TypingBar({ onBytes, onPasteText, ctrl, onCtrl }: Props) {
  const [pasting, setPasting] = useState(false);
  const [text, setText] = useState("");
  const pasteRef = useRef<HTMLTextAreaElement | null>(null);

  useEffect(() => {
    if (pasting) pasteRef.current?.focus();
  }, [pasting]);

  return (
    <div className="border-t border-border bg-card">
      {pasting ? (
        <div className="flex flex-col gap-2 p-2">
          <textarea
            value={text}
            onChange={(event) => setText(event.target.value)}
            ref={pasteRef}
            rows={3}
            placeholder="Paste here, then send"
            className="w-full resize-y rounded border border-border bg-background p-2 font-mono text-xs"
          />
          <div className="flex justify-end gap-2">
            <button
              type="button"
              onClick={() => {
                setPasting(false);
                setText("");
              }}
              className="rounded px-2 py-1 text-xs hover:bg-accent"
            >
              Cancel
            </button>
            <button
              type="button"
              disabled={text.length === 0}
              onClick={() => {
                onPasteText(text);
                setText("");
                setPasting(false);
              }}
              className="rounded bg-primary px-3 py-1 text-xs text-primary-foreground disabled:opacity-50"
            >
              Send
            </button>
          </div>
        </div>
      ) : (
        <div className="flex flex-wrap items-center gap-1 p-1">
          {ROW.map(({ key, label }) => (
            <button
              key={key}
              type="button"
              onClick={() =>
                key === "ctrl" ? onCtrl(!ctrl) : onBytes(PHONE_KEYS[key])
              }
              aria-pressed={key === "ctrl" ? ctrl : undefined}
              className={`min-w-9 rounded px-2 py-1.5 font-mono text-xs ${
                key === "ctrl" && ctrl
                  ? "bg-primary text-primary-foreground"
                  : "bg-background hover:bg-accent"
              }`}
            >
              {label}
            </button>
          ))}
          <button
            type="button"
            onClick={() => setPasting(true)}
            className="ml-auto rounded px-2 py-1.5 text-xs hover:bg-accent"
          >
            Paste…
          </button>
        </div>
      )}
    </div>
  );
}
