// Prints, as Markdown, the packages the Web UI bundles into web/dist and their licenses:
// every package of web/package-lock.json that is not a development dependency.
// Usage: node web-licenses.mjs <path to web/>
import fs from "node:fs";
import path from "node:path";

const web = process.argv[2];
if (!web) throw new Error("Usage: node web-licenses.mjs <path to web/>");
const lock = JSON.parse(fs.readFileSync(path.join(web, "package-lock.json"), "utf8"));
const bundled = Object.entries(lock.packages)
  .filter(([at, entry]) => at.includes("node_modules/") && !entry.dev && !entry.link)
  .sort(([a], [b]) => a.localeCompare(b));

const LICENSE_FILE = /^(licen[sc]e|copying)(\.(md|txt))?$/i;
for (const [at, entry] of bundled) {
  const dir = path.join(web, at);
  const manifest = JSON.parse(fs.readFileSync(path.join(dir, "package.json"), "utf8"));
  const license = manifest.license || entry.license;
  if (!license) throw new Error(`${at} declares no license`);
  const file = fs.readdirSync(dir).find((name) => LICENSE_FILE.test(name));
  if (!file) throw new Error(`${at} carries no license file`);
  const text = fs.readFileSync(path.join(dir, file), "utf8").trim();
  console.log(`### ${manifest.name} ${manifest.version}\n\n${license}\n\n\`\`\`\n${text.replaceAll("```", "'''")}\n\`\`\`\n`);
}
