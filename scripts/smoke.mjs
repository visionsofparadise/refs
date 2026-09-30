#!/usr/bin/env node
import { spawnSync } from "node:child_process";
import {
	existsSync,
	mkdirSync,
	mkdtempSync,
	readFileSync,
	realpathSync,
	renameSync,
	rmSync,
	writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { versionOf } from "./release.mjs";

const repositoryRoot = join(fileURLToPath(new URL(".", import.meta.url)), "..");
const binaryPath =
	process.argv[2] === undefined
		? join(repositoryRoot, "target", "release", process.platform === "win32" ? "refs.exe" : "refs")
		: resolve(process.argv[2]);

if (!existsSync(binaryPath)) {
	console.error(`missing artifact at ${binaryPath}`);
	process.exit(1);
}

const usage = "List file references and repair them from declared moves";
const version = versionOf();
const root = realpathSync.native(mkdtempSync(join(tmpdir(), "refs-smoke-")));

try {
	mkdirSync(join(root, "docs"));
	writeFileSync(join(root, "index.md"), "[guide](docs/guide.md)\n");
	writeFileSync(join(root, "docs", "guide.md"), "");

	const is = (expected) => ({
		description: `is ${JSON.stringify(expected)}`,
		matches: (actual) => actual === expected,
	});
	const startsWith = (expected) => ({
		description: `starts with ${JSON.stringify(expected)}`,
		matches: (actual) => actual.startsWith(expected),
	});

	const checks = [
		{
			name: "help",
			args: ["--help"],
			exitCode: is(0),
			stdout: startsWith(usage),
			stderr: is(""),
		},
		{
			name: "version",
			args: ["--version"],
			exitCode: is(0),
			stdout: is(`refs ${version}\n`),
			stderr: is(""),
		},
		{
			name: "listing finds the reference",
			args: [],
			exitCode: is(0),
			stdout: is("index.md:1:9: docs/guide.md -> docs/guide.md\n"),
			stderr: is(""),
		},
		{
			name: "piped rename rewrites the reference",
			prepare: () => {
				mkdirSync(join(root, "guides"));
				renameSync(join(root, "docs", "guide.md"), join(root, "guides", "start.md"));
			},
			input: "R\tdocs/guide.md\tguides/start.md\n",
			args: ["-"],
			exitCode: is(0),
			stdout: is("index.md:1:9: docs/guide.md -> guides/start.md\n"),
			stderr: is(""),
			file: { path: "index.md", content: is("[guide](guides/start.md)\n") },
		},
	];

	const fail = (check, comparison, expectation, actual) => {
		console.error(`${check.name}: ${comparison} ${expectation.description}`);
		console.error(`  actual ${JSON.stringify(actual)}`);
		throw new Error("check failed");
	};

	for (const check of checks) {
		check.prepare?.();

		const result = spawnSync(binaryPath, check.args, {
			cwd: root,
			input: check.input ?? "",
			encoding: "utf8",
			timeout: 60_000,
		});

		if (result.error !== undefined) {
			throw result.error;
		}

		const comparisons = [
			["exit code", check.exitCode, result.status],
			["stdout", check.stdout, result.stdout],
			["stderr", check.stderr, result.stderr],
		];

		if (check.file !== undefined) {
			comparisons.push([check.file.path, check.file.content, readFileSync(join(root, check.file.path), "utf8")]);
		}

		for (const [comparison, expectation, actual] of comparisons) {
			if (!expectation.matches(actual)) {
				fail(check, comparison, expectation, actual);
			}
		}

		console.log(`ok ${check.name}`);
	}
} catch (error) {
	if (!(error instanceof Error && error.message === "check failed")) {
		console.error(error);
	}

	process.exitCode = 1;
} finally {
	rmSync(root, { recursive: true, force: true });
}
