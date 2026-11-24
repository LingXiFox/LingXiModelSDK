import { mkdir, readdir, readFile, writeFile } from "node:fs/promises";
import { stripTypeScriptTypes } from "node:module";

const source = new URL("./src/", import.meta.url);
const output = new URL("./dist/", import.meta.url);
for (const file of await readdir(source, { recursive: true })) {
  if (!file.endsWith(".ts")) continue;
  const target = new URL(file.replace(/\.ts$/, ".js"), output);
  const input = await readFile(new URL(file, source), "utf8");
  const javascript = stripTypeScriptTypes(input, { mode: "strip" })
    .replace(/((?:from\s+|import\s*\()["']\.[^"'\n]+)\.ts(["'])/g, "$1.js$2");
  await mkdir(new URL(".", target), { recursive: true });
  await writeFile(target, javascript);
}
