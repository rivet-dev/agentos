import { readFileSync } from "node:fs";

const greeting = readFileSync(new URL("./greeting.txt", import.meta.url), "utf8");
console.log(`report: ${greeting.trim()}`);
