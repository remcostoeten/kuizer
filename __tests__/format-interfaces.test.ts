import { describe, expect, test } from "vitest"
import { fmtIface } from "../src/format-interfaces.js"
import type { THit } from "../src/find-interfaces.js"

describe("fmtIface", () => {
    test("groups by folder and file with totals", () => {
        const hits: THit[] = [
            { file: "src/a.ts", name: "A", line: 3 },
            { file: "src/a.ts", name: "B", line: 10 },
            { file: "src/dom/b.tsx", name: "C", line: 1 },
            { file: "src/dom/c.ts", name: "D", line: 7 }
        ]

        const out = fmtIface(hits)

        expect(out).toContain("interfaces: 4")
        expect(out).toContain("src  (2)")
        expect(out).toContain("src/a.ts  (2)")
        expect(out).toContain("A  line 3")
        expect(out).toContain("B  line 10")
        expect(out).toContain("src/dom  (2)")
        expect(out).toContain("src/dom/b.tsx  (1)")
        expect(out).toContain("C  line 1")
        expect(out).toContain("src/dom/c.ts  (1)")
        expect(out).toContain("D  line 7")
    })

    test("prints root for top-level files", () => {
        const hits: THit[] = [{ file: "a.ts", name: "A", line: 1 }]
        const out = fmtIface(hits)
        expect(out).toContain("root  (1)")
        expect(out).toContain("a.ts  (1)")
    })
})
