import { beforeEach, describe, expect, it } from "vitest";
import {
  DevServerUrlScanner,
  findLoopbackUrls,
  listDevServers,
  useDevServerStore,
} from "./devServers";

const bytes = (s: string) => new TextEncoder().encode(s);

describe("findLoopbackUrls", () => {
  it("finds the addresses dev servers print", () => {
    expect(
      findLoopbackUrls(
        "  ➜  Local:   http://localhost:5173/\n  ➜  Network: http://192.168.1.4:5173/",
      ),
    ).toEqual([{ port: 5173, url: "http://localhost:5173/" }]);
    expect(
      findLoopbackUrls(
        "Uvicorn running on http://127.0.0.1:8000 (Press CTRL+C)",
      ),
    ).toEqual([{ port: 8000, url: "http://127.0.0.1:8000/" }]);
    expect(findLoopbackUrls("Listening on https://[::1]:8443/api.")).toEqual([
      { port: 8443, url: "https://[::1]:8443/api" },
    ]);
    expect(findLoopbackUrls("Ready at http://localhost:3000.")).toEqual([
      { port: 3000, url: "http://localhost:3000/" },
    ]);
  });

  it("opens an all-interfaces address on localhost", () => {
    expect(findLoopbackUrls("serving at http://0.0.0.0:8080")).toEqual([
      { port: 8080, url: "http://localhost:8080/" },
    ]);
  });

  it("ignores addresses without a port, and impossible ports", () => {
    expect(
      findLoopbackUrls("see http://localhost/ and http://localhost:99999"),
    ).toEqual([]);
    expect(findLoopbackUrls("http://localhost:3000.5")).toEqual([]);
  });
});

describe("DevServerUrlScanner", () => {
  it("reads an address split across chunks once its line ends", () => {
    const scanner = new DevServerUrlScanner();
    expect(scanner.push(bytes("Local: http://localhost:51"))).toEqual([]);
    expect(scanner.push(bytes("73/\r\n"))).toEqual([
      { port: 5173, url: "http://localhost:5173/" },
    ]);
  });

  it("reads through the colors inside an address", () => {
    const scanner = new DevServerUrlScanner();
    expect(
      scanner.push(
        bytes(
          "\x1b[32m➜\x1b[39m  Local: \x1b[36mhttp://localhost:\x1b[1m5173\x1b[22m/\x1b[39m\n",
        ),
      ),
    ).toEqual([{ port: 5173, url: "http://localhost:5173/" }]);
  });

  it("reads the shown text of a hyperlink", () => {
    const scanner = new DevServerUrlScanner();
    expect(
      scanner.push(
        bytes(
          "\x1b]8;;http://localhost:3000/\x1b\\http://localhost:3000/\x1b]8;;\x1b\\\n",
        ),
      ),
    ).toEqual([{ port: 3000, url: "http://localhost:3000/" }]);
  });

  it("does not wait forever for a line that never ends", () => {
    const scanner = new DevServerUrlScanner();
    expect(
      scanner.push(bytes(`http://localhost:4321/ ${"x".repeat(5000)}`)),
    ).toEqual([{ port: 4321, url: "http://localhost:4321/" }]);
  });
});

describe("useDevServerStore", () => {
  beforeEach(() => useDevServerStore.setState({ byLeaf: new Map() }));

  it("keeps each port once per terminal and forgets them on clear", () => {
    const { add, clear } = useDevServerStore.getState();
    add(1, [{ port: 5173, url: "http://localhost:5173/" }], null);
    const before = useDevServerStore.getState().byLeaf;
    add(1, [{ port: 5173, url: "http://127.0.0.1:5173/" }], null);
    expect(useDevServerStore.getState().byLeaf).toBe(before);
    clear(1);
    expect(useDevServerStore.getState().byLeaf.size).toBe(0);
  });

  it("lists a server two terminals printed once, and tells hosts apart", () => {
    const { add } = useDevServerStore.getState();
    add(1, [{ port: 3000, url: "http://localhost:3000/" }], null);
    add(2, [{ port: 3000, url: "http://localhost:3000/" }], null);
    add(3, [{ port: 3000, url: "http://localhost:3000/" }], "dypod");
    const list = listDevServers(useDevServerStore.getState().byLeaf);
    expect(list.map((s) => [s.leafId, s.host])).toEqual([
      [1, null],
      [3, "dypod"],
    ]);
  });
});
