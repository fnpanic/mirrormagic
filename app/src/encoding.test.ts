import { readFileSync, readdirSync } from "node:fs";
import { describe, expect, it } from "vitest";

// Guards against double-encoded UTF-8 ("Ã—" instead of "×") from scripted edits.
describe("UI sources", () => {
  it("contain no mojibake", () => {
    for (const f of readdirSync("src").filter((f) => /\.tsx?$/.test(f) && !f.endsWith(".test.ts"))) {
      expect(readFileSync(`src/${f}`, "utf8"), f).not.toMatch(/Ã.|Â.|â€|âŒ/);
    }
  });
});
