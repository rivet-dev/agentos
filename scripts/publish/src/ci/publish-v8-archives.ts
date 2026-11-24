/** Promote reviewed native archives without rebuilding or changing their bytes. */
import { createHash } from "node:crypto";
import { createReadStream } from "node:fs";
import { mkdtemp, readFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { $ } from "execa";
import { uploadFile } from "../lib/r2.js";

const root = resolve(process.argv[2] ?? ".");
const source = JSON.parse(await readFile(join(root, "scripts/ci/v8/source.json"), "utf8"));
const pins = JSON.parse(await readFile(join(root, "scripts/ci/v8/archives.json"), "utf8"));
const targets = ["x86_64-unknown-linux-gnu", "aarch64-unknown-linux-gnu", "x86_64-apple-darwin", "aarch64-apple-darwin"];
if (!/^[a-f0-9]{40}$/.test(source.v8Tree)) throw new Error("Invalid V8 tree");
const work = await mkdtemp(join(tmpdir(), "agentos-v8-promotion-"));
try {
  for (const target of targets) {
    const pin = pins[target];
    const sha = typeof pin === "string" ? pin : pin?.sha256;
    if (!/^[a-f0-9]{64}$/.test(sha)) throw new Error(`Missing reviewed digest for ${target}`);
    const run = $({ cwd: root, stdio: "inherit" });
    await run`python3 scripts/ci/v8/engine.py fetch ${target} ${work} --allow-actions`;
    const archive = join(work, `${target}.tar.gz`);
    const verified = join(work, target);
    await run`python3 scripts/ci/v8/engine.py install ${target} ${verified} --bundle ${archive} --sha256 ${sha}`;
    const hash = createHash("sha256");
    for await (const chunk of createReadStream(archive)) hash.update(chunk);
    if (hash.digest("hex") !== sha) throw new Error(`Archive changed before upload: ${target}`);
    const key = `agent-os/v8/${source.v8Tree}/${sha}/${target}.tar.gz`;
    await uploadFile(archive, key);
    const response = await fetch(`https://releases.rivet.dev/${key}`, { signal: AbortSignal.timeout(120000) });
    if (!response.ok || !response.body) throw new Error(`Published archive unavailable: ${target}: ${response.status}`);
    const downloaded = createHash("sha256");
    for await (const chunk of response.body) downloaded.update(chunk);
    if (downloaded.digest("hex") !== sha) throw new Error(`Published digest mismatch: ${target}`);
    console.log(`Verified durable archive: ${target} ${sha}`);
    await rm(archive);
    await rm(verified, { recursive: true });
  }
} finally {
  await rm(work, { recursive: true, force: true });
}
