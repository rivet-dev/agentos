import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { AsyncLocalStorage } from "node:async_hooks";
import { setImmediate, setTimeout } from "node:timers";
import { Effect, Option, Stream } from "effect";
import { ChildProcess, ChildProcessSpawner } from "effect/process";
import { NodeServices } from "@effect/platform-node";

const mode = process.argv[2];
const hop = () => new Promise((resolve) => setImmediate(() => Promise.resolve().then(() => setImmediate(resolve))));
const OUT = "OUT:Ω:" + "s".repeat(8192) + ":END\n";
const ERR = "ERR:λ\n";
const source = (out, err, code) => `process.stdout.write(${JSON.stringify(out)});process.stderr.write(${JSON.stringify(err)});process.exitCode=${code};`;

function capture(out, err, code, scheduled) {
  const commits = { stdout: "", stderr: "" };
  let finalized = false;
  let finalSnapshot;
  return Effect.gen(function* () {
    const spawner = yield* ChildProcessSpawner.ChildProcessSpawner;
    const captured = yield* Effect.scoped(Effect.gen(function* () {
      yield* Effect.addFinalizer(() => Effect.sync(() => {
        finalSnapshot = { ...commits };
        finalized = true;
      }));
      const handle = yield* spawner.spawn(ChildProcess.make("node", ["-e", source(out, err, code)]));
      const collect = (stream, key) => Stream.mkString(Stream.mapEffect(Stream.decodeText(stream), (chunk) => Effect.promise(async () => {
        if (scheduled) await hop();
        commits[key] += chunk;
        return chunk;
      })));
      const [stdout, stderr, exitCode] = yield* Effect.all([
        collect(handle.stdout, "stdout"), collect(handle.stderr, "stderr"), handle.exitCode,
      ], { concurrency: "unbounded" });
      return { stdout, stderr, exitCode };
    }));
    assert(finalized);
    assert.deepEqual(finalSnapshot, { stdout: out, stderr: err }, "scope must not close before final async stream reduction");
    assert.deepEqual(captured, { stdout: out, stderr: err, exitCode: code });
    return { ...captured, finalized };
  });
}

async function effectCases(scheduled) {
  const program = Effect.gen(function* () {
    const rows = [];
    for (const [out, err, code] of [[OUT, ERR, 0], ["", "", 0], ["nonzero\n", "reason\n", 7], ["again\n", "", 0]]) {
      rows.push(yield* capture(out, err, code, scheduled));
    }
    return rows;
  });
  return Effect.runPromise(Effect.provide(program, NodeServices.layer));
}

async function cancellation() {
  const program = Effect.gen(function* () {
    const spawner = yield* ChildProcessSpawner.ChildProcessSpawner;
    let owned;
    const ready = yield* Effect.scoped(Effect.gen(function* () {
      owned = yield* spawner.spawn(ChildProcess.make("node", ["-e", "process.stdout.write('READY\\n');setInterval(()=>{},1000);"]));
      const chunk = yield* Stream.runHead(Stream.decodeText(owned.stdout));
      assert(Option.isSome(chunk), "child must be admitted and publish readiness before scope cancellation");
      return chunk.value.trim();
    }));
    assert.equal(ready, "READY");
    const runningAfterScope = yield* owned.isRunning;
    assert.equal(runningAfterScope, false, "real scoped child owner must finish cancellation");
    return { ready, runningAfterScope };
  });
  return Effect.runPromise(Effect.provide(program, NodeServices.layer));
}

async function direct() {
  const storage = new AsyncLocalStorage();
  return storage.run("child-completion-context", async () => {
    const trace = [];
    const commits = [];
    const child = spawn("node", ["-e", source("DIRECT\n", "DIRECT_ERR\n", 0)], { stdio: ["ignore", "pipe", "pipe"] });
    let stdout = "", stderr = "";
    const event = (name) => {
      assert.equal(storage.getStore(), "child-completion-context", `async context at ${name}`);
      trace.push(name);
    };
    child.once("spawn", () => event("spawn"));
    const collect = (stream, key) => new Promise((resolve, reject) => {
      stream.on("readable", () => {
        event(`${key}:readable`);
        let chunk;
        while ((chunk = stream.read()) !== null) {
          const text = Buffer.from(chunk).toString("utf8");
          commits.push(hop().then(() => { event(`${key}:commit`); if (key === "stdout") stdout += text; else stderr += text; }));
        }
      });
      stream.once("error", reject);
      stream.once("end", () => { event(`${key}:end`); resolve(); });
    });
    const streams = Promise.all([collect(child.stdout, "stdout"), collect(child.stderr, "stderr")]);
    const close = new Promise((resolve, reject) => { child.once("error", reject); child.once("exit", () => event("exit")); child.once("close", (code) => { event("close"); resolve(code); }); });
    const unrelated = Promise.all([new Promise((resolve) => setImmediate(() => { trace.push("unrelated:immediate"); resolve(); })), new Promise((resolve) => setTimeout(() => { trace.push("unrelated:timer"); resolve(); }, 0))]);
    const code = await close;
    await streams; await Promise.all(commits); await unrelated;
    assert.equal(trace.filter((x) => x === "spawn").length, 1);
    for (const name of ["stdout:readable", "stderr:readable", "stdout:end", "stderr:end", "exit", "close"]) assert(trace.indexOf("spawn") < trace.indexOf(name), `spawn before ${name}: ${JSON.stringify(trace)}`);
    assert(trace.indexOf("exit") < trace.indexOf("close"));
    assert(trace.indexOf("stdout:end") < trace.indexOf("close"));
    assert(trace.indexOf("stderr:end") < trace.indexOf("close"));
    // Native Node may publish exit before EOF and close before arbitrary async commits.
    // The owning result still awaits every consumer commit; no arbitrary commit-before-close contract is imposed.
    assert.equal(stdout, "DIRECT\n"); assert.equal(stderr, "DIRECT_ERR\n"); assert.equal(code, 0);
    return { stdout, stderr, code, contextPreserved: true, progress: true, trace };
  });
}

async function failedSpawn() {
  let spawned = false;
  const child = spawn("agentos-test-command-does-not-exist", [], { stdio: "ignore" });
  child.once("spawn", () => { spawned = true; });
  const error = await new Promise((resolve) => child.once("error", resolve));
  await hop();
  assert.equal(spawned, false); assert.equal(error.code, "ENOENT");
  return { spawned, code: error.code };
}

async function lateGuest() {
  const rows = [];
  for (let i = 0; i < 4; i++) {
    const child = spawn("node", ["-e", source(`LATE:${i}\n`, "", 0)], { stdio: ["ignore", "pipe", "pipe"] });
    await new Promise((resolve, reject) => { child.once("error", reject); child.once("close", resolve); });
    let stdout = "", chunk;
    while ((chunk = child.stdout.read()) !== null) stdout += Buffer.from(chunk).toString("utf8");
    while (child.stderr.read() !== null) assert.fail("unexpected late stderr");
    await hop();
    assert.equal(stdout, `LATE:${i}\n`); assert.equal(child.exitCode, 0);
    assert.equal(child.stdout.readableEnded, true); assert.equal(child.stderr.readableEnded, true);
    rows.push({ stdout, exitCode: child.exitCode });
  }
  return rows;
}

async function handledSpawnThrow() {
  let handled = 0, spawned = 0, closed = 0, stdout = "", stderr = "";
  const storage = new AsyncLocalStorage();
  const sentinel = "handled-spawn-listener-sentinel";
  const onUncaught = (error) => {
    if (error.message !== sentinel) throw error;
    assert.equal(storage.getStore(), "handled-spawn-context", "handled error keeps throwing child context");
    handled++;
  };
  process.on("uncaughtException", onUncaught);
  try {
    return await storage.run("handled-spawn-context", async () => {
    const child = spawn("node", ["-e", source("HANDLED_PAYLOAD\n", "HANDLED_ERR\n", 0)], { stdio: ["ignore", "pipe", "pipe"] });
    child.on("spawn", () => { spawned++; if (spawned !== 1) throw new Error("duplicate spawn"); throw new Error(sentinel); });
    child.stdout.on("data", (chunk) => { stdout += chunk.toString("utf8"); });
    child.stderr.on("data", (chunk) => { stderr += chunk.toString("utf8"); });
    const code = await new Promise((resolve, reject) => {
      child.once("error", reject);
      child.on("close", (code) => { closed++; resolve(code); });
    });
    await hop();
    assert.equal(handled, 1); assert.equal(spawned, 1); assert.equal(closed, 1);
    assert.equal(stdout, "HANDLED_PAYLOAD\n"); assert.equal(stderr, "HANDLED_ERR\n"); assert.equal(code, 0);
    return { handled, spawned, closed, stdout, stderr, code };
    });
  } finally { process.off("uncaughtException", onUncaught); }
}

let result;
if (mode === "effect") result = await effectCases(false);
else if (mode === "effect-async") result = await effectCases(true);
else if (mode === "cancel") result = await cancellation();
else if (mode === "direct") result = await direct();
else if (mode === "handled-spawn-throw") result = await handledSpawnThrow();
else if (mode === "failed-spawn") result = await failedSpawn();
else if (mode === "late-guest") result = await lateGuest();
else throw new Error(`unknown completion probe mode ${mode}`);
process.stdout.write(JSON.stringify(result) + "\n");
