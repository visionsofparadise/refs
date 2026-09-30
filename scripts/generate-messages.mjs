import { execFileSync } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, rmSync, statfsSync, writeFileSync } from "node:fs";
import { join } from "node:path";

const COREUTILS_VERSIONS = [
	"8.28",
	"8.29",
	"8.30",
	"8.31",
	"8.32",
	"9.0",
	"9.1",
	"9.2",
	"9.3",
	"9.4",
	"9.5",
	"9.6",
	"9.7",
	"9.8",
	"9.9",
	"9.10",
	"9.11",
	"9.12",
];
const GIT_VERSIONS = Array.from({ length: 56 - 20 + 1 }, (_, index) => `2.${20 + index}.0`);
const COREUTILS_SOURCE = "https://ftpmirror.gnu.org/gnu/coreutils";
const GIT_SOURCE = "https://github.com/git/git";
const CACHE = join(".scratch", "messages");
const OUTPUT = join("src", "declaration_messages.rs");
const NOTICES = "TRANSLATION-NOTICES.txt";
const CREDITS = new Map();

const MESSAGES = [
	["RENAMED", "coreutils", "renamed %s -> %s", "", 2],
	["RENAMED", "coreutils", "renamed ", "%s -> %s", 2],
	["COPIED", "coreutils", "copied %s -> %s", "", 2],
	["COPIED", "coreutils", "copied ", "%s -> %s", 2],
	["REMOVED", "coreutils", "removed %s\n", "", 1],
	["REMOVED_DIRECTORY", "coreutils", "removed directory %s\n", "", 1],
	["CREATED_DIRECTORY", "coreutils", "created directory %s", "", 1],
	["CREATED_DIRECTORY", "coreutils", "created directory %s\n", "", 1],
	["BACKUP", "coreutils", " (backup: %s)", "", 1],
	["RENAMING", "git", "Renaming %s to %s\n", "", 2],
].map(([table, tool, msgid, suffix, count]) => ({
	table,
	tool,
	msgid,
	suffix,
	count,
	templates: new Map(),
	versions: new Set(),
}));

const run = (command, args, options = {}) => execFileSync(command, args, { maxBuffer: 1 << 30, ...options });

const download = async (url, path) => {
	if (existsSync(path)) return;

	for (let attempt = 1; ; attempt++) {
		try {
			const response = await fetch(url);

			if (!response.ok) throw new Error(`${url}: ${response.status}`);

			writeFileSync(path, Buffer.from(await response.arrayBuffer()));

			return;
		} catch (error) {
			if (attempt === 3) throw error;
		}
	}
};

const decodeString = (literal) =>
	literal.replace(/\\(["\\ntr]|[0-7]{1,3})/g, (_, escape) => {
		const simple = { '"': '"', "\\": "\\", n: "\n", t: "\t", r: "\r" }[escape];

		return simple ?? String.fromCharCode(parseInt(escape, 8));
	});

const decodeCatalogue = (bytes) => {
	const head = bytes.subarray(0, 4096).toString("latin1");
	const charset = /charset=([\w-]+)/.exec(head)?.[1] ?? "UTF-8";

	return new TextDecoder(charset.toUpperCase() === "CHARSET" ? "utf-8" : charset).decode(bytes);
};

const entriesOf = (text) => {
	const entries = [];

	for (const block of text.split(/\r?\n\s*\r?\n/)) {
		const lines = block.split(/\r?\n/);
		const entry = { fuzzy: false, fields: {} };
		let field;

		for (const line of lines) {
			if (line.startsWith("#,")) {
				entry.fuzzy ||= /\bfuzzy\b/.test(line);
			} else if (line.startsWith("#")) {
				field = undefined;
			} else {
				const keyed = /^(msgctxt|msgid|msgid_plural|msgstr(?:\[\d+\])?)\s+"(.*)"\s*$/.exec(line);
				const continued = /^"(.*)"\s*$/.exec(line);

				if (keyed) {
					field = keyed[1];
					entry.fields[field] = decodeString(keyed[2]);
				} else if (continued && field) {
					entry.fields[field] += decodeString(continued[1]);
				}
			}
		}

		if (entry.fields.msgid !== undefined) entries.push(entry);
	}

	return entries;
};

const templateOf = (format, count) => {
	const literals = [""];
	const argumentsAt = [];
	let next = 0;
	const pattern = /%(?:(\d+)\$)?([s%])|%/g;
	let last = 0;
	let match;

	while ((match = pattern.exec(format))) {
		literals[literals.length - 1] += format.slice(last, match.index);
		last = pattern.lastIndex;

		if (match[2] === "%") {
			literals[literals.length - 1] += "%";
		} else if (match[2] === "s") {
			argumentsAt.push(match[1] ? Number(match[1]) - 1 : next++);
			literals.push("");
		} else {
			return undefined;
		}
	}

	literals[literals.length - 1] += format.slice(last);

	const sorted = [...argumentsAt].sort();

	if (sorted.length !== count || sorted.some((argument, index) => argument !== index)) return undefined;

	return { literals, argumentsAt };
};

const printedFormatOf = (translation, { msgid, suffix }) => {
	const printed = (msgid.endsWith("\n") ? translation.replace(/\n$/, "") : translation) + suffix;

	return printed.replace(/[ \t\r\n]+$/, "");
};

const creditsOf = (text) => {
	const lines = text.split(/\r?\n/);
	const header = lines.slice(
		0,
		lines.findIndex((line) => line.startsWith("msgid")),
	);

	return header
		.filter((line) => line.startsWith("#"))
		.map((line) => line.replace(/^#\s?/, "").trimEnd())
		.filter((line) => !/distributed under/i.test(line))
		.filter((line) => /copyright|\(c\)|©|<[^<>\s]+@[^<>\s]+>/i.test(line));
};

const collect = (messages, catalogue, skipped) => {
	const { language, version, text } = catalogue;

	for (const entry of entriesOf(text)) {
		const message = messages.find((candidate) => candidate.msgid === entry.fields.msgid);

		if (!message || entry.fields.msgctxt !== undefined) continue;

		message.versions.add(version);

		const translations = Object.entries(entry.fields)
			.filter(([key, value]) => key.startsWith("msgstr") && value !== "")
			.map(([, value]) => value);

		for (const translation of entry.fuzzy ? [] : translations) {
			const template = templateOf(printedFormatOf(translation, message), message.count);

			if (!template) {
				skipped.push(`${message.tool} ${version} ${language}: ${JSON.stringify(translation)}`);

				continue;
			}

			const key = JSON.stringify(template);
			const known = message.templates.get(key) ?? { template, languages: new Set(), versions: new Set() };

			known.languages.add(language);
			known.versions.add(version);
			message.templates.set(key, known);
			CREDITS.set(`${message.tool}/${language}`, { version, lines: creditsOf(text) });
		}
	}
};

const collectCoreutils = async (messages, skipped) => {
	const extracted = join(CACHE, "extracted");

	for (const version of COREUTILS_VERSIONS) {
		const name = `coreutils-${version}`;

		await download(`${COREUTILS_SOURCE}/${name}.tar.xz`, join(CACHE, `${name}.tar.xz`));

		rmSync(extracted, { recursive: true, force: true });
		mkdirSync(extracted);

		const members = run("tar", ["-tJf", `../${name}.tar.xz`], { cwd: extracted })
			.toString()
			.split(/\r?\n/)
			.filter((member) => /^coreutils-[\d.]+\/po\/[^/]+\.po$/.test(member));

		run("tar", ["-xJf", `../${name}.tar.xz`, ...members], { cwd: extracted });

		for (const member of members) {
			const text = decodeCatalogue(readFileSync(join(extracted, member)));
			const language = member.replace(/^.*\/(.+)\.po$/, "$1");

			collect(messages, { language, version, text }, skipped);
		}
	}

	rmSync(extracted, { recursive: true, force: true });
};

const collectGit = (messages, skipped) => {
	const repository = join(CACHE, "git.git");
	const git = (args, options) => run("git", ["-C", repository, ...args], options);

	if (!existsSync(repository)) run("git", ["init", "--bare", "--quiet", repository]);

	git(["config", "gc.auto", "0"]);

	const tags = GIT_VERSIONS.map((version) => `v${version}`);
	const missingTags = tags.filter((tag) => {
		try {
			git(["rev-parse", "--verify", "--quiet", `refs/tags/${tag}`], { stdio: "ignore" });

			return false;
		} catch {
			return true;
		}
	});

	if (missingTags.length > 0) {
		git([
			"fetch",
			"--quiet",
			"--depth=1",
			"--filter=blob:none",
			"--no-tags",
			GIT_SOURCE,
			...missingTags.map((tag) => `+refs/tags/${tag}:refs/tags/${tag}`),
		]);
	}

	const blobs = tags.flatMap((tag) =>
		git(["ls-tree", `${tag}:po`])
			.toString()
			.split("\n")
			.map((line) => /^\d+ blob ([0-9a-f]+)	(.+)\.po$/.exec(line))
			.filter(Boolean)
			.map(([, blob, language]) => ({ tag, blob, language })),
	);
	const unique = [...new Set(blobs.map(({ blob }) => blob))];
	const present = new Set(
		git(["cat-file", "--batch-check=%(objectname) %(objecttype)"], {
			input: unique.join("\n"),
			env: { ...process.env, GIT_NO_LAZY_FETCH: "1" },
		})
			.toString()
			.split("\n")
			.filter((line) => line.endsWith(" blob"))
			.map((line) => line.split(" ")[0]),
	);
	const absent = unique.filter((blob) => !present.has(blob));

	if (absent.length > 0) {
		git(
			[
				"-c",
				"fetch.negotiationAlgorithm=noop",
				"fetch",
				"--quiet",
				"--no-tags",
				"--no-write-fetch-head",
				"--stdin",
				GIT_SOURCE,
			],
			{ input: absent.join("\n") },
		);
	}

	for (const { tag, blob, language } of blobs) {
		const text = decodeCatalogue(git(["cat-file", "blob", blob]));

		collect(messages, { language, version: tag.slice(1), text }, skipped);
	}
};

const rustString = (text) =>
	`"${[...text]
		.map((character) => {
			const code = character.codePointAt(0);

			if (character === '"' || character === "\\") return `\\${character}`;
			if (character === "\n") return "\\n";
			if (character === "\t") return "\\t";
			if (code < 0x20 || code === 0x7f) return `\\u{${code.toString(16)}}`;

			return character;
		})
		.join("")}"`;

const rangeOf = (versions, all) => {
	const indices = all.flatMap((version, index) => (versions.has(version) ? [index] : []));

	if (indices.length === 0) return "none";
	if (indices.length === 1) return all[indices[0]];

	const contiguous = indices.at(-1) - indices[0] === indices.length - 1;

	return contiguous ? `${all[indices[0]]}..=${all[indices.at(-1)]}` : indices.map((index) => all[index]).join(", ");
};

const releasesOf = (tool) => (tool === "git" ? GIT_VERSIONS : COREUTILS_VERSIONS);

const tablesOf = () => {
	const tables = new Map();

	for (const message of MESSAGES) {
		const table = tables.get(message.table) ?? { tool: message.tool, messages: [], templates: new Map() };
		const english = templateOf(printedFormatOf(message.msgid, message), message.count);
		const add = (template, languages, versions) => {
			const key = JSON.stringify(template);
			const known = table.templates.get(key) ?? {
				template,
				english: false,
				languages: new Set(),
				versions: new Set(),
			};

			known.english ||= key === JSON.stringify(english);
			languages.forEach((language) => known.languages.add(language));
			versions.forEach((version) => known.versions.add(version));
			table.templates.set(key, known);
		};

		add(english, [], message.versions);

		for (const { template, languages, versions } of message.templates.values()) {
			add(template, languages, versions);
		}

		table.messages.push(message);
		tables.set(message.table, table);
	}

	return tables;
};

const render = (tables) => {
	const lines = [
		"// @generated by scripts/generate-messages.mjs; do not edit by hand.",
		"//",
		"// Every translation of the messages GNU coreutils and git print when moving or removing a",
		"// file, collected from the gettext catalogues (po/*.po) of these releases, fuzzy and empty",
		"// translations skipped:",
		`// - GNU coreutils ${COREUTILS_VERSIONS.join(", ")}: ${COREUTILS_SOURCE}/coreutils-<version>.tar.xz`,
		`// - git ${GIT_VERSIONS.join(", ")}: ${GIT_SOURCE} po/ at tag v<version>`,
		"//",
		"// A template is the literal text around each argument (`literals`, one more than the",
		"// arguments) and the printf argument each slot prints (`arguments`, 0-based), so a",
		"// translation that reorders `%1$s`/`%2$s` keeps its order. The English form comes first;",
		"// each comment names the catalogues and releases holding the template.",
		"",
		"pub struct Template {",
		"    pub literals: &'static [&'static str],",
		"    pub arguments: &'static [usize],",
		"}",
		"",
	];

	for (const [name, table] of tables) {
		const all = releasesOf(table.tool);
		const entries = [...table.templates.entries()]
			.sort(([left, a], [right, b]) => Number(b.english) - Number(a.english) || (left < right ? -1 : 1))
			.map(([, entry]) => entry);

		for (const message of table.messages) {
			lines.push(
				`/// ${table.tool} \`${JSON.stringify(message.msgid + message.suffix).slice(1, -1)}\`${message.suffix ? ` (\`${JSON.stringify(message.msgid).slice(1, -1)}\` translated)` : ""}: ${rangeOf(message.versions, all)}.`,
			);
		}

		lines.push(`pub const ${name}: &[Template] = &[`);

		for (const { template, english, languages, versions } of entries) {
			const holders = [...(english ? ["en"] : []), ...[...languages].sort()].join(", ");

			lines.push(`    // ${holders}; ${rangeOf(versions, all)}`);
			lines.push(
				`    Template { literals: &[${template.literals.map(rustString).join(", ")}], arguments: &[${template.argumentsAt.join(", ")}] },`,
			);
		}

		lines.push("];", "");
	}

	return lines.join("\n");
};

const renderNotices = () => {
	const section = (tool, title, releases, statement, sources) => {
		const languages = [...CREDITS.entries()]
			.filter(([key]) => key.startsWith(`${tool}/`))
			.map(([key, credit]) => [key.split("/")[1], credit])
			.sort(([left], [right]) => (left < right ? -1 : 1));

		return [
			title,
			"-".repeat(title.length),
			"",
			`Releases: ${releases[0]} to ${releases.at(-1)}`,
			statement,
			...sources,
			"",
			...languages.flatMap(([language, { version, lines }]) => [
				`[${language}] (header of release ${version})`,
				...lines.map((line) => `  ${line}`),
				"",
			]),
		];
	};

	return [
		"Translated message templates",
		"============================",
		"",
		"refs embeds translated message templates in src/declaration_messages.rs, so it can",
		"recognise the lines GNU coreutils and git print when they move or remove a file in any",
		"language. The templates are extracted from the gettext translation catalogues (po/*.po)",
		"of the two projects by scripts/generate-messages.mjs, and each catalogue remains under",
		"the license and copyright of its translators and the Free Software Foundation, given",
		"below.",
		"",
		...section(
			"coreutils",
			"GNU coreutils",
			COREUTILS_VERSIONS,
			'License: GPL-3.0-or-later; each catalogue header states "same license as the coreutils package".',
			[
				`Source: ${COREUTILS_SOURCE}/coreutils-<version>.tar.xz (po/)`,
				"License text: https://www.gnu.org/licenses/gpl-3.0.txt",
			],
		),
		...section(
			"git",
			"git",
			GIT_VERSIONS,
			'License: GPL-2.0-only; each catalogue header states "same license as the Git package", and its COPYING is GPL version 2.',
			[
				`Source: ${GIT_SOURCE} (po/ at tag v<version>)`,
				"License text: https://www.gnu.org/licenses/old-licenses/gpl-2.0.txt",
			],
		),
	].join("\n");
};

const main = async () => {
	const { bavail, bsize } = statfsSync(".");
	const free = Math.floor((bavail * bsize) / 2 ** 30);

	if (free < 20) throw new Error(`only ${free}G free; refusing to download`);

	mkdirSync(CACHE, { recursive: true });

	const skipped = [];

	await collectCoreutils(
		MESSAGES.filter((message) => message.tool === "coreutils"),
		skipped,
	);
	collectGit(
		MESSAGES.filter((message) => message.tool === "git"),
		skipped,
	);

	const tables = tablesOf();

	writeFileSync(OUTPUT, render(tables));
	run("rustfmt", ["--edition", "2021", OUTPUT]);
	writeFileSync(NOTICES, renderNotices());

	for (const message of MESSAGES) {
		console.log(
			`${message.table} ${JSON.stringify(message.msgid)}: ${message.templates.size} translated templates, msgid in ${rangeOf(message.versions, releasesOf(message.tool))}`,
		);
	}

	for (const [name, table] of tables) console.log(`${name}: ${table.templates.size} templates`);
	for (const line of skipped) console.log(`skipped ${line}`);
};

await main();
