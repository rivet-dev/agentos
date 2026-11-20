import { DatabaseSync } from "node:sqlite";
const quote = (value) => "'" + value.replaceAll("'", "''") + "'";
const outcome = (run) => {
  try { return { value: run() ?? null }; }
  catch (error) {
    const message = String(error.message);
    return /not authorized|authorization denied/.test(message)
      ? { denied: true } : { error: message };
  }
};
const db = new DatabaseSync(":memory:");
let file;
const result = {};
try {
  db.exec("CREATE TABLE t(a); INSERT INTO t VALUES (1)");
  result.readHostFile = outcome(() => {
    db.exec("ATTACH DATABASE " + quote(targets.secret) + " AS host");
    return db.prepare("SELECT value FROM host.secret").get().value;
  });
  result.readHostFileUri = outcome(() => db.exec("ATTACH DATABASE " + quote("file:" + targets.secret + "?mode=ro") + " AS uri"));
  result.createHostFile = outcome(() => db.exec("ATTACH DATABASE " + quote(targets.attach) + " AS created; CREATE TABLE created.t(a)"));
  result.createHostExpression = outcome(() => db.exec("ATTACH DATABASE (" + quote(targets.concat) + " || '') AS created2; CREATE TABLE created2.t(a)"));
  result.createHostParameter = outcome(() => db.prepare("ATTACH DATABASE ? AS bound").run(targets.bound));
  result.vacuumHostFile = outcome(() => db.exec("VACUUM INTO " + quote(targets.vacuum)));
  result.vacuumHostExpression = outcome(() => db.exec("VACUUM INTO (" + quote(targets.concatVacuum) + " || '')"));
  result.sharedCacheUri = outcome(() => db.exec("ATTACH DATABASE 'file:sqlite-policy-private?mode=memory&cache=shared' AS shared"));
  // Read-only probes: the old binary must not be subjected to a heap-limit setter.
  for (const name of ["data_store_directory", "hard_heap_limit", "soft_heap_limit", "temp_store_directory"]) {
    result[name] = outcome(() => db.prepare("PRAGMA " + name).all());
  }
  result.writableSchema = outcome(() => {
    db.exec("PRAGMA writable_schema = ON");
    const value = db.prepare("PRAGMA writable_schema").get().writable_schema;
    db.exec("PRAGMA writable_schema = OFF");
    return value;
  });
  result.plainVacuum = outcome(() => { db.exec("VACUUM"); return true; });
  result.memoryAttach = outcome(() => {
    db.exec("ATTACH DATABASE ':memory:' AS mem; CREATE TABLE mem.t(a); INSERT INTO mem.t VALUES (2)");
    return db.prepare("SELECT a FROM mem.t").get().a;
  });
  result.temporaryAttach = outcome(() => {
    db.exec("ATTACH DATABASE '' AS temporary; CREATE TABLE temporary.t(a); INSERT INTO temporary.t VALUES (4)");
    return db.prepare("SELECT a FROM temporary.t").get().a;
  });
  result.localPragma = outcome(() => {
    db.exec("PRAGMA user_version = 7");
    return db.prepare("PRAGMA user_version").get().user_version;
  });
  result.vmFile = outcome(() => {
    file = new DatabaseSync("/tmp/in-vm-policy.sqlite");
    file.exec("CREATE TABLE t(a); INSERT INTO t VALUES (3)");
    file.exec("VACUUM");
    file.close();
    file = new DatabaseSync("/tmp/in-vm-policy.sqlite");
    return file.prepare("SELECT a FROM t").get().a;
  });
  result.stillUsable = outcome(() => db.prepare("SELECT a FROM main.t").get().a);
} finally {
  try { if (file) file.close(); } finally { db.close(); }
}
console.log(JSON.stringify(result, (_key, value) =>
  typeof value === "bigint" ? { sqliteBigInt: value.toString(10) } : value,
));
