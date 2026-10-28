import fs from "node:fs/promises";
import path from "node:path";
const destination = path.resolve("src-tauri/resources/collector");
await fs.mkdir(destination, { recursive: true });
await fs.copyFile(
  process.execPath,
  path.join(destination, process.platform === "win32" ? "node.exe" : "node"),
);
await fs.copyFile(
  path.join(path.dirname(process.execPath), "LICENSE"),
  path.join(destination, "NODE-LICENSE"),
);
await fs.cp("scripts/kaoyan", path.join(destination, "scripts"), {
  recursive: true,
});
console.log("采集运行环境已准备");
