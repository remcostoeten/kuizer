import { describe, expect, test } from "vitest"
import { readArg } from "../src/args.js"

describe("readArg", () => {
    test("defaults to dry mode with no args", () => {
        const result = readArg([])
        expect(result).toEqual({
            dry: true,
            apply: false,
            revert: null,
            browse: false,
            listIface: false
        })
    })

    test("--list-interfaces sets listIface flag", () => {
        const result = readArg(["--list-interfaces"])
        expect(result.listIface).toBe(true)
    })

    test("--apply sets apply mode and disables dry", () => {
        const result = readArg(["--apply"])
        expect(result.apply).toBe(true)
        expect(result.dry).toBe(false)
    })

    test("--dry sets dry mode and disables apply", () => {
        const result = readArg(["--dry"])
        expect(result.dry).toBe(true)
        expect(result.apply).toBe(false)
    })

    test("--browse sets browse flag", () => {
        const result = readArg(["--browse"])
        expect(result.browse).toBe(true)
    })

    test("--revert with no value defaults to latest", () => {
        const result = readArg(["--revert"])
        expect(result.revert).toBe("latest")
    })

    test("--revert with value sets specific id", () => {
        const result = readArg(["--revert", "20250125-123456-abc"])
        expect(result.revert).toBe("20250125-123456-abc")
    })

    test("--revert= syntax works", () => {
        const result = readArg(["--revert=20250125-123456-abc"])
        expect(result.revert).toBe("20250125-123456-abc")
    })

    test("--revert= with empty value defaults to latest", () => {
        const result = readArg(["--revert="])
        expect(result.revert).toBe("latest")
    })

    test("--revert followed by another flag defaults to latest", () => {
        const result = readArg(["--revert", "--browse"])
        expect(result.revert).toBe("latest")
        expect(result.browse).toBe(true)
    })

    test("multiple flags can be combined", () => {
        const result = readArg(["--browse", "--dry"])
        expect(result.browse).toBe(true)
        expect(result.dry).toBe(true)
    })

    test("last mode flag wins (apply after dry)", () => {
        const result = readArg(["--dry", "--apply"])
        expect(result.apply).toBe(true)
        expect(result.dry).toBe(false)
    })

    test("last mode flag wins (dry after apply)", () => {
        const result = readArg(["--apply", "--dry"])
        expect(result.apply).toBe(false)
        expect(result.dry).toBe(true)
    })
})
