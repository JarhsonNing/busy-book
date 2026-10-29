import fs from "node:fs/promises";
import path from "node:path";
const nodeDirectory = path.dirname(await fs.realpath(process.execPath));
const licenseCandidates = [
  path.join(nodeDirectory, "LICENSE"),
  path.resolve(nodeDirectory, "..", "LICENSE"),
];
let nodeLicense;
for (const candidate of licenseCandidates) {
  try {
    nodeLicense = await fs.readFile(candidate);
    break;
  } catch (error) {
    if (error.code !== "ENOENT") throw error;
  }
}
if (!nodeLicense) {
  throw new Error(`未找到 Node.js 许可证，已检查：${licenseCandidates.join(", ")}`);
}
const destination = path.resolve("src-tauri/resources/collector");
await fs.mkdir(destination, { recursive: true });
await fs.copyFile(
  process.execPath,
  path.join(destination, process.platform === "win32" ? "node.exe" : "node"),
);
await fs.writeFile(path.join(destination, "NODE-LICENSE"), nodeLicense);
await fs.cp("scripts/kaoyan", path.join(destination, "scripts"), {
  recursive: true,
});
console.log("采集运行环境已准备");
