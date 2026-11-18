import { describe, expect, it } from "vitest";
import {
  applyReplayEvents,
  buildAttachHandoff,
  buildCommitHandoff,
  buildDetachHandoff,
  buildPrepareHandoff,
  buildRollbackHandoff,
  decodeReplayOutput,
  nextHandoffId,
} from "./ptyHandoff";

describe("ptyHandoff", () => {
  it("keeps u64 values as decimal strings in the prepare payload", () => {
    expect(buildPrepareHandoff("18446744073709551615", [7], "space-a")).toEqual(
      {
        handoffId: "18446744073709551615",
        ids: [7],
        to: "space-a",
      },
    );
    expect(buildDetachHandoff("9", [7, 8])).toEqual({
      handoffId: "9",
      ids: [7, 8],
    });
    expect(buildAttachHandoff("9", [7])).toEqual({ handoffId: "9", ids: [7] });
    expect(buildRollbackHandoff("9", [7])).toEqual({
      handoffId: "9",
      ids: [7],
    });
  });

  it("mints handoff ids the backend can parse as u64", () => {
    const a = nextHandoffId(1_794_976_123_706);
    const b = nextHandoffId(1_794_976_123_706);
    for (const id of [a, b, nextHandoffId()]) {
      expect(id).toMatch(/^\d+$/);
      expect(Number.isSafeInteger(Number(id))).toBe(true);
    }
    expect(a).not.toBe(b);
  });

  it("passes each lease token and epoch through as strings", () => {
    const leases = [
      {
        ptyId: 4,
        attachToken: "18446744073709551615",
        leaseEpoch: "3",
      },
    ];
    expect(buildCommitHandoff("9", leases)).toEqual({
      handoffId: "9",
      leases,
    });
  });

  it("decodes base64 output and applies resize in order", () => {
    const written: Uint8Array[] = [];
    const resized: Array<[number, number]> = [];
    applyReplayEvents(
      {
        write: (bytes) => written.push(bytes),
        resize: (cols, rows) => resized.push([cols, rows]),
      },
      [
        { kind: "output", data: btoa("hello") },
        { kind: "resize", cols: 100, rows: 30 },
        { kind: "output", data: btoa("!") },
      ],
    );
    expect(written.map((bytes) => new TextDecoder().decode(bytes))).toEqual([
      "hello",
      "!",
    ]);
    expect(resized).toEqual([[100, 30]]);
    expect(decodeReplayOutput(btoa("x"))).toEqual(new Uint8Array([120]));
  });

  it("refuses replays that cannot be applied to a real terminal", () => {
    const model = { write: () => {}, resize: () => {} };
    expect(() =>
      applyReplayEvents(model, [{ kind: "resize", cols: 0, rows: 30 }]),
    ).toThrow();
    expect(() =>
      applyReplayEvents(model, [{ kind: "resize", cols: 80, rows: -1 }]),
    ).toThrow();
  });

  it("refuses replay output that is not base64", () => {
    expect(() => decodeReplayOutput("not base64!")).toThrow();
  });
});
