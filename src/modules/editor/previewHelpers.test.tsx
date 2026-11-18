import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { CsvPreview, csvSummary } from "./CsvPreview";
import { fileNameOf, ImagePreview } from "./ImagePreview";
import { isDelimitedPath } from "./lib/csvPreview";
import {
  isSvgPath,
  mediaPreviewKind,
  svgDataUrl,
} from "./lib/mediaPreview";

describe("csv summary", () => {
  it("counts data rows without the header", () => {
    expect(csvSummary(3, 2, false)).toBe("3 rows · 2 columns");
    expect(csvSummary(1, 2, false)).toBe("1 row · 2 columns");
  });

  it("says the rows are only the first ones when the preview is cut", () => {
    expect(csvSummary(200, 4, true)).toBe("First 200 rows · 4 columns");
    expect(csvSummary(120, 4, false, "48.0 MB")).toBe(
      "First 120 rows · 4 columns · of a 48.0 MB file",
    );
  });

  it("renders the count from the parsed table and hides Edit without a handler", () => {
    const html = renderToStaticMarkup(
      createElement(CsvPreview, { content: "a,b\n1,2\n3,4\n", delimiter: "," }),
    );
    expect(html).toContain("2 rows · 2 columns");
    expect(html).not.toContain("<button");
  });

  it("recognises delimited files by extension", () => {
    expect(isDelimitedPath("D:\\data\\Big.CSV")).toBe(true);
    expect(isDelimitedPath("/tmp/x.tsv")).toBe(true);
    expect(isDelimitedPath("/tmp/x.txt")).toBe(false);
  });
});

describe("image preview", () => {
  it("takes the file name from Windows and POSIX paths", () => {
    expect(fileNameOf("D:\\pics\\logo.png")).toBe("logo.png");
    expect(fileNameOf("/home/u/logo.png")).toBe("logo.png");
  });

  it("shows the image with its size and the zoom and grid controls", () => {
    const html = renderToStaticMarkup(
      createElement(ImagePreview, {
        src: "asset://localhost/logo.png",
        name: "logo.png",
        sizeLabel: "12.0 KB",
      }),
    );
    expect(html).toContain('src="asset://localhost/logo.png"');
    expect(html).toContain('alt="logo.png"');
    expect(html).toContain("12.0 KB");
    for (const label of ["Fit", "100%", "Grid"]) {
      expect(html).toContain(`>${label}</button>`);
    }
  });
});

describe("svg preview", () => {
  it("treats .svg as an image and encodes the buffer as a data URL", () => {
    expect(isSvgPath("C:\\art\\Icon.SVG")).toBe(true);
    expect(isSvgPath("icon.svg.txt")).toBe(false);
    expect(mediaPreviewKind("icon.svg")).toBe("image");
    const url = svgDataUrl('<svg xmlns="http://www.w3.org/2000/svg">#é</svg>');
    expect(url.startsWith("data:image/svg+xml;charset=utf-8,")).toBe(true);
    // "#" would otherwise start a fragment and cut the document short.
    expect(url).not.toContain("#");
    expect(decodeURIComponent(url.split(",")[1])).toContain("#é</svg>");
  });
});
