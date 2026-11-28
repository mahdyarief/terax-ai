import { useEffect, useRef, useState } from "react";
import { composerKeyAction } from "./composer";
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
  /** Sends the text as one write (bracketed when the program asked). */
  onText: (text: string) => void;
  onCtrlLetter: (letter: string) => void;
  ctrl: boolean;
  onCtrl: (next: boolean) => void;
};

/**
 * Keys a phone keyboard does not have, and a typing area under them that is
 * always open while typing is on: Enter sends the line, Shift+Enter adds a
 * new one, and a paste lands in it like any text. Every key is an Escape
 * sequence from Ghostty's own table.
 */
export function TypingBar({
  onBytes,
  onText,
  onCtrlLetter,
  ctrl,
  onCtrl,
}: Props) {
  const [text, setText] = useState("");
  const areaRef = useRef<HTMLTextAreaElement | null>(null);

  useEffect(() => {
    areaRef.current?.focus();
  }, []);

  const submit = () => {
    if (text.length > 0) onText(text);
    onBytes(PHONE_KEYS.enter);
    setText("");
    areaRef.current?.focus();
  };

  return (
    <div className="border-t border-border bg-card">
      <div className="flex flex-wrap items-center gap-1 p-1">
        {ROW.map(({ key, label }) => (
          <button
            key={key}
            type="button"
            // Keep the typing area focused so a phone keyboard stays open.
            onMouseDown={(event) => event.preventDefault()}
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
      </div>
      <div className="flex items-end gap-2 px-2 pb-2">
        <textarea
          ref={areaRef}
          value={text}
          onChange={(event) => setText(event.target.value)}
          onKeyDown={(event) => {
            const action = composerKeyAction(
              {
                key: event.key,
                shiftKey: event.shiftKey,
                composing: event.nativeEvent.isComposing,
              },
              text,
              ctrl,
            );
            if (!action) return;
            event.preventDefault();
            if (action.kind === "submit") submit();
            else if (action.kind === "ctrl") {
              onCtrlLetter(action.letter);
              onCtrl(false);
            } else onBytes(action.bytes);
          }}
          rows={Math.min(6, Math.max(1, text.split("\n").length))}
          aria-label="Type a command"
          placeholder="Type a command, Enter to send"
          autoCapitalize="none"
          autoCorrect="off"
          spellCheck={false}
          enterKeyHint="send"
          className="min-h-9 w-full resize-none rounded border border-border bg-background px-2 py-2 font-mono text-xs outline-none focus:border-primary"
        />
        <button
          type="button"
          onMouseDown={(event) => event.preventDefault()}
          onClick={submit}
          className="h-9 shrink-0 rounded bg-primary px-3 text-xs text-primary-foreground"
        >
          Send
        </button>
      </div>
    </div>
  );
}
