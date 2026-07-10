'use client';

import { useEffect, useId, useMemo, useLayoutEffect, useRef, useState } from 'react';
import { FileCode2, FileText, Play, RotateCcw, SquarePen } from 'lucide-react';
import { AnimatePresence, motion, useReducedMotion } from 'framer-motion';
import { InkPanel } from '../editorial/InkPanel';

// ---------------------------------------------------------------------------
// Recorded agent coding sessions, played back line by line inside an ink
// terminal. A segmented control picks the runtime and replays the same task
// with the agent writing that language; the active runtime's description and
// docs link sit under the control and swap with it. A filesystem rail on the
// right shows /home/agentos changing as the session runs, so the script
// visibly works against a whole OS rather than a chat box. The Python session
// runs the interpreter natively (no shell lines); Node and bash keep their
// commands.
// ---------------------------------------------------------------------------

type SessionLineKind = 'user' | 'agent' | 'cmd' | 'run' | 'out' | 'script';

interface SessionLine {
	kind: SessionLineKind;
	text: string;
}

type ScriptLang = 'js' | 'python' | 'bash';

interface SessionFile {
	name: string;
	size: string;
	kind: 'code' | 'data';
	// Session line index that creates this file; it appears in the rail once
	// that line has finished playing.
	afterIndex: number;
}

interface SessionTab {
	key: string;
	title: string;
	description: string;
	docsHref: string;
	docsLabel: string;
	iconSrc: string;
	script: { fileName: string; lang: ScriptLang; code: string };
	session: SessionLine[];
	files: SessionFile[];
}

// One task, three languages: fetch last week's issues, roll them up, write
// report.md. The label counts sum to the reported total (9+6+5+3 = 23), and
// the bash tab lists them alphabetically because its jq pipeline sorts.
const REPORT_JS = `import { writeFileSync } from "node:fs";

const since = new Date(Date.now() - 7 * 864e5).toISOString();
const url = \`https://api.github.com/repos/acme/shop/issues?since=\${since}\`;
const issues = await (await fetch(url)).json();

const counts = {};
for (const { labels } of issues)
  for (const { name } of labels) counts[name] = (counts[name] ?? 0) + 1;

const rows = Object.entries(counts).sort((a, b) => b[1] - a[1]);
writeFileSync("report.md", [
  \`# Issues, last 7 days: \${issues.length}\`,
  ...rows.map(([label, n]) => \`- \${label}: \${n}\`),
].join("\\n"));`;

const REPORT_PY = `import json, datetime, urllib.request
from collections import Counter

since = (datetime.date.today() - datetime.timedelta(days=7)).isoformat()
url = f"https://api.github.com/repos/acme/shop/issues?since={since}"
issues = json.load(urllib.request.urlopen(url))

counts = Counter(l["name"] for i in issues for l in i["labels"])

lines = [f"# Issues, last 7 days: {len(issues)}"]
lines += [f"- {label}: {n}" for label, n in counts.most_common()]
report = "\\n".join(lines)
open("report.md", "w").write(report)
print(report)`;

const REPORT_SH = `#!/bin/bash
set -euo pipefail

since=$(date -u -d '7 days ago' +%Y-%m-%d)
url="https://api.github.com/repos/acme/shop/issues?since=$since"
curl -s "$url" > issues.json

{
  echo "# Issues, last 7 days: $(jq length issues.json)"
  jq -r '[.[].labels[].name] | sort | group_by(.)
         | .[] | "- \\(.[0]): \\(length)"' issues.json
} > report.md`;

const REPORT_OUT = ['- bug: 9', '- api: 6', '- ui: 5', '- docs: 3'];

const shellSession = (runCmd: string, reportLines: string[]): SessionLine[] => [
	{ kind: 'user', text: "generate a report of last week's issues" },
	{ kind: 'agent', text: 'Writing a script to fetch them and build the report.' },
	{ kind: 'script', text: '' },
	{ kind: 'cmd', text: runCmd },
	{ kind: 'cmd', text: 'cat report.md' },
	{ kind: 'out', text: '# Issues, last 7 days: 23' },
	...reportLines.map((text): SessionLine => ({ kind: 'out', text })),
	{ kind: 'agent', text: '23 issues last week; bug reports lead with 9.' },
];

// Python runs on the native interpreter: the run step replaces shell lines
// and the script prints the report it just wrote.
const pythonSession: SessionLine[] = [
	{ kind: 'user', text: "generate a report of last week's issues" },
	{ kind: 'agent', text: 'Writing a script to fetch them and build the report.' },
	{ kind: 'script', text: '' },
	{ kind: 'run', text: 'Run report.py' },
	{ kind: 'out', text: '# Issues, last 7 days: 23' },
	...REPORT_OUT.map((text): SessionLine => ({ kind: 'out', text })),
	{ kind: 'agent', text: '23 issues last week; bug reports lead with 9.' },
];

const TABS: SessionTab[] = [
	{
		key: 'nodejs',
		title: 'JavaScript & TypeScript',
		description: 'Node v22 compatible on native V8 isolates. node, npm, and npx on the PATH at full JIT speed.',
		docsHref: '/docs/nodejs-runtime',
		docsLabel: 'Node.js runtime docs',
		iconSrc: '/images/registry/nodejs.svg',
		script: { fileName: 'report.mjs', lang: 'js', code: REPORT_JS },
		session: shellSession('node report.mjs', REPORT_OUT),
		files: [
			{ name: 'report.mjs', size: '486 B', kind: 'code', afterIndex: 2 },
			{ name: 'report.md', size: '96 B', kind: 'data', afterIndex: 3 },
		],
	},
	{
		key: 'python',
		title: 'Python',
		description: 'CPython 3.13 with pip. Native wheels like numpy and pandas work.',
		docsHref: '/docs/python-runtime',
		docsLabel: 'Python runtime docs',
		iconSrc: '/images/registry/python.svg',
		script: { fileName: 'report.py', lang: 'python', code: REPORT_PY },
		session: pythonSession,
		files: [
			{ name: 'report.py', size: '451 B', kind: 'code', afterIndex: 2 },
			{ name: 'report.md', size: '96 B', kind: 'data', afterIndex: 3 },
		],
	},
	{
		key: 'bash',
		title: 'Linux shell',
		description: 'A POSIX userland with a process table, PTYs, TCP and UDP with DNS, and deny-by-default permissions.',
		docsHref: '/docs/architecture',
		docsLabel: 'Kernel & shell docs',
		iconSrc: '/images/registry/linux.svg',
		script: { fileName: 'report.sh', lang: 'bash', code: REPORT_SH },
		session: shellSession('bash report.sh', ['- api: 6', '- bug: 9', '- docs: 3', '- ui: 5']),
		files: [
			{ name: 'report.sh', size: '338 B', kind: 'code', afterIndex: 2 },
			{ name: 'issues.json', size: '18 KB', kind: 'data', afterIndex: 3 },
			{ name: 'report.md', size: '96 B', kind: 'data', afterIndex: 3 },
		],
	},
];

const WINDOW_TITLE = 'agentos vm · /home/agentos';
const CAPTION = 'The agent writes one program instead of a chain of tool calls.';

// --- Tiny dark-palette tokenizer for the script block ---------------------
// The site's highlightCodeHtml is tuned for light panels and JS only; the
// script hero sits on ink and covers three languages, so it gets its own
// minimal pass: comments, strings, keywords, and shell variables.

type TokenType = 'kw' | 'str' | 'com' | 'var' | 'text';

interface Token {
	type: TokenType;
	value: string;
}

const TOKEN_CLASS: Record<Exclude<TokenType, 'text'>, string> = {
	kw: 'text-sage',
	str: 'text-[#CFA379]',
	com: 'italic text-cream/40',
	var: 'text-[#CFA379]',
};

const SCRIPT_RULES: Record<ScriptLang, RegExp> = {
	js: /(\/\/.*)|(`(?:\\.|[^`])*`|"(?:\\.|[^"])*"|'(?:\\.|[^'])*')|()\b(import|from|const|let|var|await|async|function|return|new|for|of|if|else|export)\b/g,
	python:
		/(#.*)|((?:[frbFRB]{1,2})?(?:"(?:\\.|[^"])*"|'(?:\\.|[^'])*'))|()\b(import|from|def|return|for|in|if|else|with|as|class|lambda)\b/g,
	bash: /(#.*)|("(?:\\.|[^"])*"|'[^']*')|(\$\{?\w+\}?)|\b(set|echo|if|then|else|fi|for|do|done)\b/g,
};

// Tokenizes the whole script (strings may span lines), then splits into
// per-line token runs for rendering.
const tokenizeScript = (code: string, lang: ScriptLang): Token[][] => {
	const rule = new RegExp(SCRIPT_RULES[lang].source, 'g');
	const tokens: Token[] = [];
	let last = 0;
	for (let match = rule.exec(code); match; match = rule.exec(code)) {
		if (match.index > last) tokens.push({ type: 'text', value: code.slice(last, match.index) });
		const [, com, str, shVar, kw] = match;
		const type: TokenType = com !== undefined ? 'com' : str !== undefined ? 'str' : shVar ? 'var' : kw ? 'kw' : 'text';
		tokens.push({ type, value: match[0] });
		last = match.index + match[0].length;
	}
	if (last < code.length) tokens.push({ type: 'text', value: code.slice(last) });

	const lines: Token[][] = [[]];
	for (const token of tokens) {
		token.value.split('\n').forEach((part, idx) => {
			if (idx > 0) lines.push([]);
			if (part) lines[lines.length - 1].push({ type: token.type, value: part });
		});
	}
	return lines;
};

// --- Playback --------------------------------------------------------------
// Pacing in clock ms. `user` and `cmd` lines type per character; the rest
// appear whole after their lead-in. The script keeps the longest pause so the
// eye can land on it; everything else is brisk.
const START_DELAY = 250;
const RUN_DELAY = 350;
const LEAD: Record<SessionLineKind, number> = { user: 200, agent: 550, cmd: 480, run: 600, out: 60, script: 700 };
const CHAR_MS: Partial<Record<SessionLineKind, number>> = { user: 17, cmd: 8 };

interface ScheduledLine extends SessionLine {
	start: number;
	dur: number;
}

const buildSchedule = (lines: SessionLine[]): { schedule: ScheduledLine[]; total: number } => {
	let t = START_DELAY;
	const schedule = lines.map((line, i) => {
		const prev = lines[i - 1]?.kind;
		const lead = line.kind === 'out' && (prev === 'cmd' || prev === 'run') ? RUN_DELAY : LEAD[line.kind];
		const start = t + lead;
		const dur = (CHAR_MS[line.kind] ?? 0) * line.text.length;
		t = start + dur;
		return { ...line, start, dur };
	});
	return { schedule, total: t };
};

// Drives the playback clock with an accumulated per-frame delta (capped so a
// backgrounded tab resumes where it paused instead of jumping to the end).
// The reset runs in a layout effect so a tab switch never flashes the new
// transcript fully-played before the clock restarts.
const usePlaybackClock = (total: number, running: boolean, playKey: number, skipToEnd: boolean) => {
	const [clock, setClock] = useState(skipToEnd ? total : 0);

	useLayoutEffect(() => {
		if (skipToEnd) {
			setClock(total);
			return;
		}
		if (!running) return;
		setClock(0);
		let raf = 0;
		let last = performance.now();
		let elapsed = 0;
		const step = (now: number) => {
			elapsed += Math.min(now - last, 100);
			last = now;
			setClock(elapsed);
			if (elapsed < total) raf = requestAnimationFrame(step);
		};
		raf = requestAnimationFrame(step);
		return () => cancelAnimationFrame(raf);
	}, [total, running, playKey, skipToEnd]);

	return clock;
};

const Cursor = ({ blink }: { blink?: boolean }) => (
	<span
		aria-hidden='true'
		className={`-mb-0.5 ml-px inline-block h-[1.05em] w-[7px] translate-y-[2px] bg-cream/80 ${
			blink ? 'animate-[session-cursor-blink_1.1s_steps(2,jump-none)_infinite]' : ''
		}`}
	/>
);

// The hero: the program the agent wrote, whole and syntax-lit, under a
// "Write <file>" header. Deliberately not typed out.
const ScriptBlock = ({ fileName, tokenLines }: { fileName: string; tokenLines: Token[][] }) => (
	<div className='my-1.5 ml-[1.35rem] overflow-hidden rounded-lg border border-cream/[0.14] bg-cream/[0.045]'>
		<div className='flex items-center gap-1.5 border-b border-cream/10 px-3 py-1.5 text-[11px] text-cream/60'>
			<SquarePen aria-hidden='true' className='h-3 w-3 text-cream/40' />
			Write {fileName}
		</div>
		<pre className='overflow-x-auto px-3 py-2.5 text-[12px] leading-[1.65] text-cream/85'>
			{tokenLines.map((tokens, i) => (
				<div key={i} className='whitespace-pre'>
					{tokens.length === 0
						? ' '
						: tokens.map((token, j) =>
								token.type === 'text' ? (
									token.value
								) : (
									<span key={j} className={TOKEN_CLASS[token.type]}>
										{token.value}
									</span>
								),
							)}
				</div>
			))}
		</pre>
	</div>
);

// A native tool step ("Run report.py"): the interpreter runs in the VM
// directly, so there is no shell line to type.
const RunBlock = ({ text }: { text: string }) => (
	<div className='my-1.5 ml-[1.35rem] flex w-fit items-center gap-1.5 rounded-lg border border-cream/[0.14] bg-cream/[0.045] px-3 py-1.5 text-[11px] text-cream/60'>
		<Play aria-hidden='true' className='h-3 w-3 text-sage' />
		{text}
	</div>
);

// One transcript row. Typed rows render a partial slice with the cursor at
// the write head; whole rows fade in on mount.
const SessionRow = ({
	line,
	chars,
	typing,
	script,
	tokenLines,
}: {
	line: ScheduledLine;
	chars: number;
	typing: boolean;
	script: SessionTab['script'];
	tokenLines: Token[][];
}) => {
	const typed = line.dur > 0;
	const text = typed ? line.text.slice(0, chars) : line.text;

	const body = (() => {
		switch (line.kind) {
			case 'user':
				return (
					<span className='text-cream'>
						<span aria-hidden='true' className='mr-2.5 select-none text-accent'>
							›
						</span>
						{text}
						{typing && <Cursor />}
					</span>
				);
			case 'agent':
				return (
					<span className='text-cream/80'>
						<span
							aria-hidden='true'
							className='mr-2.5 -mt-px inline-block h-[7px] w-[7px] rounded-full bg-sage align-middle'
						/>
						{text}
					</span>
				);
			case 'cmd':
				return (
					<span className='text-cream/90'>
						<span aria-hidden='true' className='mr-2.5 select-none text-sage'>
							$
						</span>
						{text}
						{typing && <Cursor />}
					</span>
				);
			case 'run':
				return <RunBlock text={line.text} />;
			case 'out':
				return <span className='block whitespace-pre-wrap pl-[1.35rem] text-cream/45'>{text}</span>;
			case 'script':
				return <ScriptBlock fileName={script.fileName} tokenLines={tokenLines} />;
		}
	})();

	if (typed) return <div>{body}</div>;
	return (
		<motion.div initial={{ opacity: 0 }} animate={{ opacity: 1 }} transition={{ duration: 0.18 }}>
			{body}
		</motion.div>
	);
};

// The filesystem rail: /home/agentos, updated as the session writes files.
// Files created this session carry a sage dot, the way an editor marks fresh
// changes.
const FileRail = ({ files, schedule, clock }: { files: SessionFile[]; schedule: ScheduledLine[]; clock: number }) => {
	const visibleFiles = files.filter((file) => {
		const line = schedule[file.afterIndex];
		return line ? clock >= line.start + line.dur + 120 : false;
	});
	return (
		<aside className='hidden border-l border-cream/10 md:block'>
			<div className='border-b border-cream/10 px-4 py-2.5 font-mono text-[11px] text-cream/45'>/home/agentos</div>
			<div className='flex flex-col gap-0.5 p-3'>
				<div className='flex items-center gap-2 rounded-md px-2 py-1.5 text-[12px] text-cream/35'>
					<FileText aria-hidden='true' className='h-3.5 w-3.5 shrink-0' />
					<span className='truncate font-mono'>.profile</span>
				</div>
				{visibleFiles.map((file) => (
					<motion.div
						key={file.name}
						initial={{ opacity: 0, x: 6 }}
						animate={{ opacity: 1, x: 0 }}
						transition={{ duration: 0.25, ease: [0.22, 1, 0.36, 1] }}
						className='flex items-center gap-2 rounded-md bg-cream/[0.04] px-2 py-1.5 text-[12px] text-cream/80'
					>
						{file.kind === 'code' ? (
							<FileCode2 aria-hidden='true' className='h-3.5 w-3.5 shrink-0 text-sage' />
						) : (
							<FileText aria-hidden='true' className='h-3.5 w-3.5 shrink-0 text-cream/50' />
						)}
						<span className='truncate font-mono'>{file.name}</span>
						<span className='ml-auto shrink-0 font-mono text-[10px] text-cream/40'>{file.size}</span>
						<span aria-hidden='true' className='h-1.5 w-1.5 shrink-0 rounded-full bg-sage/80' />
					</motion.div>
				))}
			</div>
		</aside>
	);
};

// The runtime cards are the tabs: real buttons with no interactive content
// nested inside them. The active runtime's docs link lives in the terminal's
// caption instead, so a card is one target that does one thing.
// A segmented control (same vocabulary as the benchmark workload toggle, so
// it unambiguously reads as a switch) plus the active runtime's one-line
// description with its docs link inline, swapping with the selection.
const RuntimeTabs = ({ active, onChange }: { active: number; onChange: (idx: number) => void }) => {
	const indicatorId = useId();
	const tab = TABS[active];
	return (
		<div className='mb-5'>
			<div
				role='tablist'
				aria-label='Execution runtimes'
				className='flex w-fit flex-wrap gap-1 rounded-lg border border-ink/10 bg-white/55 p-1'
			>
				{TABS.map((t, idx) => {
					const selected = active === idx;
					return (
						<button
							key={t.key}
							type='button'
							role='tab'
							aria-selected={selected}
							onClick={() => onChange(idx)}
							className={`relative flex items-center gap-2 rounded-md px-3 py-1.5 text-sm font-medium transition-colors ${
								selected ? 'text-cream' : 'text-ink-soft hover:text-ink'
							}`}
						>
							{selected && (
								<motion.span
									layoutId={`runtime-tab-${indicatorId}`}
									className='absolute inset-0 rounded-md bg-ink'
									transition={{ type: 'spring', stiffness: 480, damping: 38 }}
								/>
							)}
							<span className='relative z-[1] flex items-center gap-2'>
								<img
									src={t.iconSrc}
									alt=''
									aria-hidden='true'
									className={`h-4 w-4 object-contain transition-[filter] ${selected ? 'brightness-0 invert' : ''}`}
								/>
								{t.title}
							</span>
						</button>
					);
				})}
			</div>
			<div className='mt-3 min-h-[2.75rem] sm:min-h-[1.5rem]'>
				<AnimatePresence mode='wait' initial={false}>
					<motion.p
						key={tab.key}
						initial={{ opacity: 0, y: 4 }}
						animate={{ opacity: 1, y: 0 }}
						exit={{ opacity: 0, y: -4 }}
						transition={{ duration: 0.18, ease: 'easeOut' }}
						className='max-w-4xl text-sm leading-relaxed text-ink-soft'
					>
						{tab.description}{' '}
						<a
							href={tab.docsHref}
							className='whitespace-nowrap text-accent-deep underline underline-offset-2 transition-colors hover:text-accent'
						>
							{tab.docsLabel} <span aria-hidden='true'>→</span>
						</a>
					</motion.p>
				</AnimatePresence>
			</div>
		</div>
	);
};

export const AgentSessionDemo = () => {
	const reduced = useReducedMotion() ?? false;
	const [active, setActive] = useState(0);
	const [started, setStarted] = useState(false);
	const [playKey, setPlayKey] = useState(0);
	const scrollRef = useRef<HTMLDivElement>(null);

	const tab = TABS[active];
	const { schedule, total } = useMemo(() => buildSchedule(tab.session), [tab]);
	const tokenLines = useMemo(() => tokenizeScript(tab.script.code, tab.script.lang), [tab]);
	const clock = usePlaybackClock(total, started, playKey, reduced);

	const visible = schedule.filter((line) => clock >= line.start);
	const done = clock >= total;

	const replay = () => {
		setStarted(true);
		setPlayKey((k) => k + 1);
	};

	const handleTabChange = (idx: number) => {
		setActive(idx);
		replay();
	};

	// Keep the newest line in view while the session plays. Instant, not
	// smooth: smooth scrolling lags behind per-frame typing updates.
	useEffect(() => {
		const el = scrollRef.current;
		if (el) el.scrollTop = el.scrollHeight;
	}, [visible.length, clock, done]);

	return (
		<motion.div
			onViewportEnter={() => setStarted(true)}
			viewport={{ once: true, margin: '-20% 0px' }}
		>
			<RuntimeTabs active={active} onChange={handleTabChange} />

			<InkPanel caption={CAPTION}>
				<div className='flex items-center gap-2 border-b border-cream/10 px-4 py-3'>
					<div className='h-3 w-3 rounded-full bg-cream/15' />
					<div className='h-3 w-3 rounded-full bg-cream/15' />
					<div className='h-3 w-3 rounded-full bg-cream/15' />
					<span className='ml-2 font-mono text-xs text-cream/45'>{WINDOW_TITLE}</span>
					{!reduced && (
						<button
							type='button'
							onClick={replay}
							aria-label='Replay session'
							className='ml-auto flex h-7 w-7 items-center justify-center rounded-md text-cream/40 transition-colors hover:bg-cream/[0.06] hover:text-cream/80'
						>
							<RotateCcw className='h-3.5 w-3.5' />
						</button>
					)}
				</div>

				<div className='md:grid md:grid-cols-[minmax(0,1fr),240px]'>
					{/* Tall enough on desktop to hold a whole session, script and all; if
					    wrapped lines overflow on small screens the auto-scroll keeps the
					    newest line in view. Ligatures off: a terminal shows `--` and
					    `->` as raw ASCII. */}
					<div
						ref={scrollRef}
						className='h-[480px] overflow-y-auto p-6 font-code text-[13px] leading-[1.75] [font-variant-ligatures:none] md:h-[640px]'
					>
						<div className='flex flex-col gap-0.5'>
							{visible.map((line, i) => {
								const typing = line.dur > 0 && clock < line.start + line.dur;
								const chars = line.dur > 0 ? Math.ceil(((clock - line.start) / line.dur) * line.text.length) : line.text.length;
								return (
									<SessionRow
										key={`${tab.key}-${i}`}
										line={line}
										chars={Math.min(chars, line.text.length)}
										typing={typing}
										script={tab.script}
										tokenLines={tokenLines}
									/>
								);
							})}
							{/* Idle prompt once the session finishes: the machine is still there. */}
							{done && !reduced && (
								<motion.div initial={{ opacity: 0 }} animate={{ opacity: 1 }} transition={{ duration: 0.18, delay: 0.5 }}>
									<span aria-hidden='true' className='mr-2.5 select-none text-sage'>
										$
									</span>
									<Cursor blink />
								</motion.div>
							)}
						</div>
					</div>

					<FileRail files={tab.files} schedule={schedule} clock={clock} />
				</div>
			</InkPanel>
		</motion.div>
	);
};
