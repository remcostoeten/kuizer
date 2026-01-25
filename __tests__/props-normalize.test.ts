import fs from "fs"
import os from "os"
import path from "path"
import { describe, expect, test } from "vitest"
import { fixPropsFile } from "../src/fix-props.js"

function mkTmp() {
    const dir = fs.mkdtempSync(path.join(os.tmpdir(), "kuizer-"))
    const src = path.join(dir, "src")
    fs.mkdirSync(src, { recursive: true })

    fs.writeFileSync(
        path.join(dir, "tsconfig.json"),
        JSON.stringify(
            {
                compilerOptions: {
                    target: "ES2022",
                    module: "ESNext",
                    moduleResolution: "Bundler",
                    strict: true
                },
                include: ["src/**/*.ts", "src/**/*.tsx"]
            },
            null,
            2
        )
    )

    return { dir, src }
}

function run(input: string, ext: ".ts" | ".tsx" = ".tsx") {
    const { dir, src } = mkTmp()
    const file = path.join(src, `file${ext}`)
    fs.writeFileSync(file, input, "utf8")

    const out = fixPropsFile({
        root: dir,
        tsCfg: path.join(dir, "tsconfig.json"),
        filePath: file
    })

    return out
}

describe("props normalize", () => {
    test("renames single local type alias to Props", () => {
        const input = [
            `import React from "react";`,
            ``,
            `type SomeProps = { a: number };`,
            ``,
            `export function X(p: SomeProps) {`,
            `  return <div>{p.a}</div>;`,
            `}`
        ].join("\n")

        const exp = [
            `import React from "react";`,
            ``,
            `type Props = { a: number };`,
            ``,
            `export function X(p: Props) {`,
            `  return <div>{p.a}</div>;`,
            `}`
        ].join("\n")

        expect(run(input)).toBe(exp)
    })

    test("renames single local type alias named props to Props", () => {
        const input = [
            `import React from "react";`,
            ``,
            `type props = { a: number };`,
            ``,
            `export function X(p: props) {`,
            `  return <div>{p.a}</div>;`,
            `}`
        ].join("\n")

        const exp = [
            `import React from "react";`,
            ``,
            `type Props = { a: number };`,
            ``,
            `export function X(p: Props) {`,
            `  return <div>{p.a}</div>;`,
            `}`
        ].join("\n")

        expect(run(input)).toBe(exp)
    })

    test("converts single local interface to type Props", () => {
        const input = [
            `import React from "react";`,
            ``,
            `interface Blabla {`,
            `  a: number;`,
            `}`,
            ``,
            `export function X(p: Blabla) {`,
            `  return <div>{p.a}</div>;`,
            `}`
        ].join("\n")

        const exp = [
            `import React from "react";`,
            ``,
            `type Props = {`,
            `  a: number;`,
            `};`,
            ``,
            `export function X(p: Props) {`,
            `  return <div>{p.a}</div>;`,
            `}`
        ].join("\n")

        expect(run(input)).toBe(exp)
    })

    test("does not rename exported type alias even if it is the only type", () => {
        const input = [
            `import React from "react";`,
            ``,
            `export type SomeProps = { a: number };`,
            ``,
            `export function X(p: SomeProps) {`,
            `  return <div>{p.a}</div>;`,
            `}`
        ].join("\n")

        expect(run(input)).toBe(input)
    })

    test("renames only the single local type when there is also an exported type", () => {
        const input = [
            `import React from "react";`,
            ``,
            `type SomeProps = { a: number };`,
            `export type SomeExportedProps = { b: number };`,
            ``,
            `export function X(p: SomeProps) {`,
            `  return <div>{p.a}</div>;`,
            `}`
        ].join("\n")

        const exp = [
            `import React from "react";`,
            ``,
            `type Props = { a: number };`,
            `export type SomeExportedProps = { b: number };`,
            ``,
            `export function X(p: Props) {`,
            `  return <div>{p.a}</div>;`,
            `}`
        ].join("\n")

        expect(run(input)).toBe(exp)
    })

    test("does not rename when there are two local types", () => {
        const input = [
            `import React from "react";`,
            ``,
            `type A = { a: number };`,
            `type B = { b: number };`,
            ``,
            `export function X(p: A) {`,
            `  return <div>{p.a}</div>;`,
            `}`
        ].join("\n")

        expect(run(input)).toBe(input)
    })

    test("does not rename when there is one local type and one local interface", () => {
        const input = [
            `import React from "react";`,
            ``,
            `type A = { a: number };`,
            `interface B {`,
            `  b: number;`,
            `}`,
            ``,
            `export function X(p: A) {`,
            `  return <div>{p.a}</div>;`,
            `}`
        ].join("\n")

        expect(run(input)).toBe(input)
    })
})
