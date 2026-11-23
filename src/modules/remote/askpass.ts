import { create } from "zustand";

// OpenSSH's questions while connecting to a server (passwords, host keys),
// asked through `terax-cli` and the control server. Answers live only in
// the dialog and the reply; they are never stored or logged.

export type AskpassKind =
  | "secret"
  | { yes_no: { host_key: boolean } }
  | "changed_host_key"
  | "confirm"
  | "notice";

export type AskpassQuestion = {
  id: string;
  host: string;
  prompt: string;
  kind: AskpassKind;
};

export type AskpassAnswer = { answer: string; confirmed: boolean };

export class AskpassParamsError extends Error {}

/** Validates what the control server sent; anything else is refused. */
export function parseAskpassQuestion(
  id: string,
  params: unknown,
): AskpassQuestion {
  if (typeof params !== "object" || params === null) {
    throw new AskpassParamsError("askpass parameters are required");
  }
  const value = params as Record<string, unknown>;
  if (typeof value.host !== "string" || value.host.length === 0) {
    throw new AskpassParamsError("askpass host is required");
  }
  if (typeof value.prompt !== "string") {
    throw new AskpassParamsError("askpass prompt is required");
  }
  const kind = value.kind;
  const simple = ["secret", "changed_host_key", "confirm", "notice"];
  const yesNo =
    typeof kind === "object" &&
    kind !== null &&
    typeof (kind as { yes_no?: { host_key?: unknown } }).yes_no?.host_key ===
      "boolean";
  if (!(typeof kind === "string" && simple.includes(kind)) && !yesNo) {
    throw new AskpassParamsError("askpass kind is unknown");
  }
  return {
    id,
    host: value.host,
    prompt: value.prompt,
    kind: kind as AskpassKind,
  };
}

type Pending = {
  question: AskpassQuestion;
  resolve: (answer: AskpassAnswer | null) => void;
};

type State = {
  queue: Pending[];
  /** Resolves with the answer, or `null` when dismissed. */
  ask: (question: AskpassQuestion) => Promise<AskpassAnswer | null>;
  /** Answers the question on screen (the first in the queue). */
  answer: (answer: AskpassAnswer | null) => void;
};

export const useAskpassStore = create<State>((set, get) => ({
  queue: [],
  ask: (question) =>
    new Promise((resolve) => {
      set((state) => ({ queue: [...state.queue, { question, resolve }] }));
    }),
  answer: (answer) => {
    const [current, ...rest] = get().queue;
    if (!current) return;
    set({ queue: rest });
    current.resolve(answer);
  },
}));

export function isHostKeyQuestion(kind: AskpassKind): boolean {
  return typeof kind === "object" && kind.yes_no.host_key;
}
