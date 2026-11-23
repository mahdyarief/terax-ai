import { checkReadable } from "@/modules/ai/lib/security";
import { previewBoundary } from "@/modules/markdown/lib/assetPath";
import { describe, expect, it, vi } from "vitest";
import { inlineRelativeAssets, maybeInlineHtmlAssets } from "./inlineHtmlAssets";

const enc = (s: string) =>
  typeof btoa === "function"
    ? btoa(unescape(encodeURIComponent(s)))
    : Buffer.from(s, "utf8").toString("base64");

function makeFs(files: Record<string, string | { base64: string }>) {
  return {
    readText: vi.fn(async (path: string) => {
      const v = files[path];
      if (v == null) throw new Error(`missing ${path}`);
      if (typeof v === "string") return v;
      throw new Error("binary");
    }),
    readBinary: vi.fn(async (path: string) => {
      const v = files[path];
      if (v == null) throw new Error(`missing ${path}`);
      if (typeof v === "string") return enc(v);
      return v.base64;
    }),
  };
}

describe("inlineRelativeAssets", () => {
  it("inlines a relative stylesheet in the same folder", async () => {
    const fs = makeFs({
      "/proj/styles.css": "body{color:red}",
    });
    const html = '<html><head><link rel="stylesheet" href="styles.css"></head></html>';
    const out = await inlineRelativeAssets(html, {
      basePath: "/proj/index.html",
      ...fs,
    });
    expect(out).not.toContain('<link rel="stylesheet" href="styles.css">');
    expect(out).toContain("<style");
    expect(out).toContain("body{color:red}");
  });

  it("resolves nested css url() and @import relative to the css file", async () => {
    const fs = makeFs({
      "/proj/css/main.css": '@import "parts/a.css"; .bg{background:url(../img/p.png)}',
      "/proj/css/parts/a.css": ".a{color:blue}",
      "/proj/img/p.png": { base64: "aWNvbg==" },
    });
    const html = '<link rel="stylesheet" href="css/main.css">';
    const out = await inlineRelativeAssets(html, {
      basePath: "/proj/index.html",
      ...fs,
    });
    expect(out).toContain(".a{color:blue}");
    expect(out).toContain("data:image/png;base64,aWNvbg==");
    expect(fs.readText).toHaveBeenCalledWith("/proj/css/main.css");
    expect(fs.readText).toHaveBeenCalledWith("/proj/css/parts/a.css");
  });

  it("leaves remote and data urls untouched", async () => {
    const fs = makeFs({});
    const html =
      '<link rel="stylesheet" href="https://cdn/x.css"><img src="data:image/png;base64,abc">';
    const out = await inlineRelativeAssets(html, {
      basePath: "/proj/index.html",
      ...fs,
    });
    expect(out).toContain('href="https://cdn/x.css"');
    expect(out).toContain('src="data:image/png;base64,abc"');
    expect(fs.readText).not.toHaveBeenCalled();
  });

  it("inlines relative images as data urls", async () => {
    const fs = makeFs({ "/proj/pic.png": { base64: "QUJD" } });
    const html = '<img src="./pic.png">';
    const out = await inlineRelativeAssets(html, {
      basePath: "/proj/index.html",
      ...fs,
    });
    expect(out).toContain('src="data:image/png;base64,QUJD"');
  });

  it("rewrites url() in style attributes", async () => {
    const fs = makeFs({ "/proj/bg.jpg": { base64: "QkNH" } });
    const html = '<div style="background:url(bg.jpg)"></div>';
    const out = await inlineRelativeAssets(html, {
      basePath: "/proj/index.html",
      ...fs,
    });
    expect(out).toContain("data:image/jpeg;base64,QkNH");
  });

  it("refuses to inline assets that escape the document folder boundary", async () => {
    const fs = makeFs({
      "/proj/outside.png": { base64: "T1VU" },
      "/proj/sub/inside.png": { base64: "SU4=" },
    });
    const html = '<img src="../outside.png"><img src="inside.png">';
    const out = await inlineRelativeAssets(html, {
      basePath: "/proj/sub/index.html",
      ...fs,
    });
    // ../outside.png resolves above the doc folder and must stay untouched.
    expect(out).toContain('src="../outside.png"');
    expect(out).toContain('src="data:image/png;base64,SU4="');
    expect(fs.readBinary).not.toHaveBeenCalledWith("/proj/outside.png");
  });

  it("allows escaping the doc folder when the workspace boundary permits it", async () => {
    const fs = makeFs({ "/proj/shared/x.css": "body{margin:0}" });
    const html = '<link rel="stylesheet" href="../shared/x.css">';
    const out = await inlineRelativeAssets(html, {
      basePath: "/proj/app/index.html",
      boundary: "/proj",
      ...fs,
    });
    expect(out).toContain("body{margin:0}");
  });

  it("never inlines secret files even inside the boundary", async () => {
    const fs = makeFs({ "/proj/.env": "API_KEY=abc", "/proj/app.js": "x" });
    const html = '<img src=".env"><link rel="stylesheet" href=".env">';
    const out = await inlineRelativeAssets(html, {
      basePath: "/proj/index.html",
      ...fs,
    });
    expect(out).not.toContain("API_KEY=abc");
    expect(fs.readText).not.toHaveBeenCalled();
    expect(fs.readBinary).not.toHaveBeenCalled();
  });

  it("honours an explicit isReadable policy", async () => {
    const fs = makeFs({ "/proj/x.css": "body{color:red}" });
    const html = '<link rel="stylesheet" href="x.css">';
    const out = await inlineRelativeAssets(html, {
      basePath: "/proj/index.html",
      isReadable: (abs) => abs !== "/proj/x.css",
      ...fs,
    });
    expect(out).toContain('href="x.css"');
    expect(out).not.toContain("body{color:red}");
  });
});

describe("a page in a folder on a server", () => {
  // Server paths are POSIX whatever this machine is; the files are read
  // through the workspace's backend, so only the scope decides here.
  const root = "/home/dev/app";
  const page = `${root}/site/index.html`;
  const scope = {
    boundary: previewBoundary(page, root),
    isReadable: (abs: string) => checkReadable(abs).ok,
  };

  it("inlines a relative stylesheet and nothing outside the folder or secret", async () => {
    const fs = makeFs({
      [`${root}/site/css/site.css`]: "body{color:teal}",
      [`${root}/.ssh/keys.css`]: "LEAKED_KEY",
      "/home/dev/other.css": "LEAKED_SIBLING",
    });
    const html = [
      '<link rel="stylesheet" href="css/site.css">',
      '<link rel="stylesheet" href="../../other.css">',
      '<link rel="stylesheet" href="../.ssh/keys.css">',
    ].join("");
    const out = await maybeInlineHtmlAssets(html, page, fs, scope);
    expect(out).toContain("body{color:teal}");
    expect(out).not.toContain("LEAKED");
    expect(fs.readText).toHaveBeenCalledWith(`${root}/site/css/site.css`);
    expect(fs.readText).not.toHaveBeenCalledWith("/home/dev/other.css");
    expect(fs.readText).not.toHaveBeenCalledWith(`${root}/.ssh/keys.css`);
  });
});
