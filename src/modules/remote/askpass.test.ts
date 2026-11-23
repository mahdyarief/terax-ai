import { describe, expect, it } from "vitest";
import {
  AskpassParamsError,
  parseAskpassQuestion,
  useAskpassStore,
} from "./askpass";

describe("askpass questions", () => {
  it("accept what the control server sends", () => {
    expect(
      parseAskpassQuestion("q1", {
        host: "dypod",
        prompt: "me@dypod's password: ",
        kind: "secret",
      }),
    ).toEqual({
      id: "q1",
      host: "dypod",
      prompt: "me@dypod's password: ",
      kind: "secret",
    });
    expect(
      parseAskpassQuestion("q2", {
        host: "dypod",
        prompt: "?",
        kind: { yes_no: { host_key: true } },
      }).kind,
    ).toEqual({ yes_no: { host_key: true } });
  });

  it("refuse anything else", () => {
    for (const params of [
      null,
      "x",
      { prompt: "p", kind: "secret" },
      { host: "h", kind: "secret" },
      { host: "h", prompt: "p", kind: "password" },
      { host: "h", prompt: "p", kind: { yes_no: {} } },
    ]) {
      expect(() => parseAskpassQuestion("q", params)).toThrow(
        AskpassParamsError,
      );
    }
  });

  it("are answered one at a time, in order", async () => {
    const store = useAskpassStore.getState();
    const q = (id: string) => ({
      id,
      host: "h",
      prompt: "p",
      kind: "secret" as const,
    });
    const first = store.ask(q("a"));
    const second = store.ask(q("b"));
    expect(useAskpassStore.getState().queue.map((p) => p.question.id)).toEqual([
      "a",
      "b",
    ]);
    useAskpassStore.getState().answer({ answer: "one", confirmed: false });
    useAskpassStore.getState().answer(null);
    await expect(first).resolves.toEqual({ answer: "one", confirmed: false });
    await expect(second).resolves.toBeNull();
    expect(useAskpassStore.getState().queue).toEqual([]);
  });
});
