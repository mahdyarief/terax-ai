import { describe, expect, it } from "vitest";
import {
  decodeFrames,
  encodeRequest,
  encodeRequestWithEpoch,
  MAX_ACKS_PER_REQUEST,
  PtyWireError,
  tokenBytes,
} from "@/modules/terminal/lib/ptyWire";
import { decodeRequest, encodeFrames } from "./ptyWireFixtures";

describe("PTY stream wire format", () => {
  const token = tokenBytes("0123456789abcdef");

  it("encodes the token, lease epoch, and count little-endian", () => {
    expect([...token]).toEqual([
      0xef, 0xcd, 0xab, 0x89, 0x67, 0x45, 0x23, 0x01,
    ]);
    const acks = [
      { id: 1, bytes: 0 },
      { id: 0xffff_ffff, bytes: 2 ** 40 + 12345 },
      { id: 7, bytes: Number.MAX_SAFE_INTEGER },
    ];
    const body = encodeRequestWithEpoch(
      token,
      "18446744073709551615",
      acks,
    );
    expect(body.length).toBe(20 + 3 * 12);
    expect([...body.subarray(8, 16)]).toEqual(Array(8).fill(0xff));
    expect(new DataView(body.buffer).getUint32(16, true)).toBe(3);
    expect(decodeRequest(body)).toEqual({
      token: "0123456789abcdef",
      epoch: 0xffff_ffff_ffff_ffffn,
      acks,
    });
  });

  it("defaults the lease epoch to zero for the current stream API", () => {
    expect(decodeRequest(encodeRequest(token, []))).toEqual({
      token: "0123456789abcdef",
      epoch: 0n,
      acks: [],
    });
  });

  it("rejects values native would refuse", () => {
    expect(() => tokenBytes("zz")).toThrow(PtyWireError);
    expect(() => tokenBytes("0123456789abcde")).toThrow(PtyWireError);
    expect(() => encodeRequest(token.subarray(1), [])).toThrow(PtyWireError);
    expect(() => encodeRequestWithEpoch(token, "-1", [])).toThrow(PtyWireError);
    expect(() =>
      encodeRequestWithEpoch(token, "18446744073709551616", []),
    ).toThrow(PtyWireError);
    expect(() => encodeRequest(token, [{ id: 1, bytes: -1 }])).toThrow(
      PtyWireError,
    );
    expect(() => encodeRequest(token, [{ id: 1, bytes: 1.5 }])).toThrow(
      PtyWireError,
    );
    expect(() =>
      encodeRequest(token, [{ id: 1, bytes: Number.MAX_SAFE_INTEGER + 1 }]),
    ).toThrow(PtyWireError);
    const tooMany = Array.from(
      { length: MAX_ACKS_PER_REQUEST + 1 },
      (_, id) => ({
        id,
        bytes: 0,
      }),
    );
    expect(() => encodeRequest(token, tooMany)).toThrow(PtyWireError);
    expect(
      encodeRequest(token, tooMany.slice(0, MAX_ACKS_PER_REQUEST)).length,
    ).toBeLessThan(1024);
  });

  it("decodes every frame kind in order across terminals", () => {
    const body = encodeFrames([
      { kind: "data", id: 3, offset: 0, bytes: Uint8Array.of(104, 105) },
      { kind: "data", id: 4, offset: 2 ** 33 + 5, bytes: Uint8Array.of(1) },
      { kind: "sync", id: 4 },
      { kind: "failed", id: 5, reason: "reader failed ☃" },
      { kind: "exit", id: 3, code: -1 },
    ]);
    expect(decodeFrames(body)).toEqual([
      { kind: "data", id: 3, offset: 0, bytes: Uint8Array.of(104, 105) },
      { kind: "data", id: 4, offset: 2 ** 33 + 5, bytes: Uint8Array.of(1) },
      { kind: "sync", id: 4 },
      { kind: "failed", id: 5, reason: "reader failed ☃" },
      { kind: "exit", id: 3, code: -1 },
    ]);
    expect(decodeFrames(new Uint8Array(0))).toEqual([]);
  });

  it("decodes frames out of a view without copying the payload", () => {
    const body = encodeFrames([
      { kind: "data", id: 1, offset: 0, bytes: Uint8Array.of(9, 8, 7) },
    ]);
    const padded = new Uint8Array(body.length + 5);
    padded.set(body, 3);
    const [frame] = decodeFrames(padded.subarray(3, 3 + body.length));
    expect(frame.kind).toBe("data");
    if (frame.kind === "data") {
      expect(frame.bytes.buffer).toBe(padded.buffer);
      expect([...frame.bytes]).toEqual([9, 8, 7]);
    }
  });

  it("treats any malformed byte as stream desynchronisation", () => {
    const good = encodeFrames([
      { kind: "data", id: 1, offset: 0, bytes: Uint8Array.of(1, 2) },
      { kind: "exit", id: 1, code: 0 },
    ]);
    for (let length = 1; length < good.length; length++) {
      // Every strict prefix cuts a frame short, except at a frame boundary.
      const prefix = good.subarray(0, length);
      if (length === 9 + 8 + 2) {
        expect(decodeFrames(prefix)).toHaveLength(1);
      } else {
        expect(() => decodeFrames(prefix), `prefix ${length}`).toThrow(
          PtyWireError,
        );
      }
    }
    const unknown = good.slice();
    unknown[4] = 99;
    expect(() => decodeFrames(unknown)).toThrow(/unknown frame kind/);
    const emptyData = encodeFrames([
      { kind: "data", id: 1, offset: 0, bytes: new Uint8Array(0) },
    ]);
    expect(() => decodeFrames(emptyData)).toThrow(/empty data frame/);
    const hugeOffset = encodeFrames([
      { kind: "data", id: 1, offset: 2 ** 53, bytes: Uint8Array.of(1) },
    ]);
    expect(() => decodeFrames(hugeOffset)).toThrow(/out of range/);
    const badReason = encodeFrames([{ kind: "failed", id: 1, reason: "x" }]);
    badReason[9] = 0xff;
    expect(() => decodeFrames(badReason)).toThrow(/reason/);
    const syncWithPayload = encodeFrames([{ kind: "sync", id: 1 }]);
    const longer = new Uint8Array(syncWithPayload.length + 1);
    longer.set(syncWithPayload);
    new DataView(longer.buffer).setUint32(5, 1, true);
    expect(() => decodeFrames(longer)).toThrow(/sync/);
  });
});
