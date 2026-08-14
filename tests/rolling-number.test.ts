import { describe, expect, it } from "vitest";
import { splitRollingValue } from "../src/components/RollingNumber";

describe("rolling number glyphs", () => {
  it("keeps units and separators while extracting digit targets", () => {
    expect(splitRollingValue("$187.3M")).toEqual([
      { kind: "symbol", value: "$" },
      { kind: "digit", value: 1 },
      { kind: "digit", value: 8 },
      { kind: "digit", value: 7 },
      { kind: "symbol", value: "." },
      { kind: "digit", value: 3 },
      { kind: "symbol", value: "M" },
    ]);
  });

  it("supports duration and percentage labels", () => {
    expect(splitRollingValue("3h 33m").filter(glyph => glyph.kind === "digit").map(glyph => glyph.value)).toEqual([3, 3, 3]);
    expect(splitRollingValue("79% remaining").slice(0, 3)).toEqual([
      { kind: "digit", value: 7 },
      { kind: "digit", value: 9 },
      { kind: "symbol", value: "%" },
    ]);
  });
});
