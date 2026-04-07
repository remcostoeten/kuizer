import { describe, expect, it } from "bun:test";
import {
	detect as detectAllUnusedExports,
	detectUnusedClasses,
	detectUnusedComponents,
	detectUnusedConstants,
	detectUnusedDefaults,
	detectUnusedEnums,
	detectUnusedFunctions,
	detectUnusedOthers,
	detectUnusedTypes,
} from "../../src/detectors/unused-exports.js";
import { makeConfig, makeCtx } from "./fixture.js";

describe("unused export split detectors", () => {
	it("partitions aggregate unused exports into non-overlapping detector buckets", () => {
		const ctx = makeCtx(
			{
				"/src/functions.ts": "export function deadFn() {}\n",
				"/src/types.ts": [
					"export type DeadType = string",
					"export interface DeadInterface { value: string }",
				].join("\n"),
				"/src/classes.ts": "export class DeadClass {}\n",
				"/src/components.tsx": [
					"export function DeadWidget() { return null }",
					"export class DeadPanel { render() { return null } }",
				].join("\n"),
				"/src/constants.ts": "export const DEAD_CONST = 1\n",
				"/src/enums.ts": "export enum DeadEnum { A }\n",
				"/src/default.ts": "export default function DeadDefault() { return null }\n",
				"/src/others.ts": "const hidden = 1\nexport { hidden }\n",
			},
			{
				"/src/functions.ts": "export function deadFn() {}\n",
				"/src/types.ts": [
					"export type DeadType = string",
					"export interface DeadInterface { value: string }",
				].join("\n"),
				"/src/classes.ts": "export class DeadClass {}\n",
				"/src/components.tsx": [
					"export function DeadWidget() { return null }",
					"export class DeadPanel { render() { return null } }",
				].join("\n"),
				"/src/constants.ts": "export const DEAD_CONST = 1\n",
				"/src/enums.ts": "export enum DeadEnum { A }\n",
				"/src/default.ts": "export default function DeadDefault() { return null }\n",
				"/src/others.ts": "const hidden = 1\nexport { hidden }\n",
			},
		);
		const config = makeConfig();

		const aggregate = detectAllUnusedExports(ctx, config);
		const splitFindings = [
			...detectUnusedFunctions(ctx, config),
			...detectUnusedTypes(ctx, config),
			...detectUnusedClasses(ctx, config),
			...detectUnusedComponents(ctx, config),
			...detectUnusedConstants(ctx, config),
			...detectUnusedEnums(ctx, config),
			...detectUnusedDefaults(ctx, config),
			...detectUnusedOthers(ctx, config),
		];

		expect(aggregate.map((finding) => finding.symbol).sort((a, b) => a.localeCompare(b))).toEqual(
			splitFindings.map((finding) => finding.symbol).sort((a, b) => a.localeCompare(b)),
		);
		expect(new Set(splitFindings.map((finding) => `${finding.file}:${finding.symbol}`)).size).toBe(
			splitFindings.length,
		);

		expect(detectUnusedFunctions(ctx, config).map((finding) => finding.symbol)).toEqual(["deadFn"]);
		expect(detectUnusedTypes(ctx, config).map((finding) => finding.symbol).sort((a, b) => a.localeCompare(b))).toEqual([
			"DeadInterface",
			"DeadType",
		]);
		expect(detectUnusedClasses(ctx, config).map((finding) => finding.symbol)).toEqual(["DeadClass"]);
		expect(detectUnusedComponents(ctx, config).map((finding) => finding.symbol).sort((a, b) => a.localeCompare(b))).toEqual([
			"DeadPanel",
			"DeadWidget",
		]);
		expect(detectUnusedConstants(ctx, config).map((finding) => finding.symbol)).toEqual([
			"DEAD_CONST",
		]);
		expect(detectUnusedEnums(ctx, config).map((finding) => finding.symbol)).toEqual(["DeadEnum"]);
		expect(detectUnusedDefaults(ctx, config).map((finding) => finding.symbol)).toEqual(["default"]);
		expect(detectUnusedOthers(ctx, config).map((finding) => finding.symbol)).toEqual(["hidden"]);
	});

	it("classifies PascalCase TSX exports as components, not generic functions or classes", () => {
		const ctx = makeCtx(
			{
				"/src/components.tsx": [
					"export function HeroCard() { return null }",
					"export class SidebarPanel { render() { return null } }",
					"export function helper() { return null }",
				].join("\n"),
			},
			{
				"/src/components.tsx": [
					"export function HeroCard() { return null }",
					"export class SidebarPanel { render() { return null } }",
					"export function helper() { return null }",
				].join("\n"),
			},
		);
		const config = makeConfig();

		expect(detectUnusedComponents(ctx, config).map((finding) => finding.symbol).sort((a, b) => a.localeCompare(b))).toEqual([
			"HeroCard",
			"SidebarPanel",
		]);
		expect(detectUnusedFunctions(ctx, config).map((finding) => finding.symbol)).toEqual(["helper"]);
		expect(detectUnusedClasses(ctx, config)).toHaveLength(0);
	});

	it("keeps detector-specific severities while aggregate unusedExports uses its own severity", () => {
		const ctx = makeCtx(
			{
				"/src/functions.ts": "export function deadFn() {}\n",
				"/src/types.ts": "export type DeadType = string\n",
			},
			{
				"/src/functions.ts": "export function deadFn() {}\n",
				"/src/types.ts": "export type DeadType = string\n",
			},
		);
		const config = makeConfig({
			detectors: {
				unusedExports: { severity: "warn" },
				unusedFunctions: { severity: "error" },
				unusedTypes: { severity: "info" },
			},
		});

		expect(detectAllUnusedExports(ctx, config).map((finding) => finding.severity)).toEqual([
			"warn",
			"warn",
		]);
		expect(detectUnusedFunctions(ctx, config)[0]?.severity).toBe("error");
		expect(detectUnusedTypes(ctx, config)[0]?.severity).toBe("info");
	});
});
