// Isolated HTTP acceptance: no credentials or real marketplaces are used.
import assert from "node:assert/strict";
import fs from "node:fs/promises";
import path from "node:path";
import os from "node:os";
import http from "node:http";
import { spawn } from "node:child_process";
import { once } from "node:events";
const root = await fs.mkdtemp(path.join(os.tmpdir(), "taoshu-redesign-"));
const port = 18993,
  origin = "http://127.0.0.1:" + port;
const html =
  "<h1>2027年研究生招生</h1><p>初试考试范围</p><p>829 数据工程基础</p><p>【参考教材】王珊, 萨师煊.数据库系统概论（第五版）.高等教育出版社，2014（第1-11章）</p>";
let slow = false,
  fail = false;
const fixture = http.createServer((req, res) => {
  const send = () => {
    res.writeHead(fail ? 500 : 200, {
      "content-type": "text/html;charset=utf-8",
    });
    res.end(fail ? "fixture failure" : html);
  };
  if (slow) setTimeout(send, 2500);
  else send();
});
fixture.listen(18994, "127.0.0.1");
await once(fixture, "listening");
const executable = path.resolve("src-tauri/target/debug/api-server.exe");
let server, token;
const wait = (ms) => new Promise((r) => setTimeout(r, ms));
async function start() {
  server = spawn(executable, [], {
    env: { ...process.env, TAOSHU_DATA: root, PORT: String(port) },
    windowsHide: true,
    stdio: "ignore",
  });
  for (let i = 0; i < 60; i++) {
    try {
      const r = await fetch(origin + "/api/token");
      token = (await r.json()).token;
      return;
    } catch {
      await wait(100);
    }
  }
  throw Error("Test server unavailable");
}
async function stop() {
  const process = server;
  process.kill();
  await once(process, "exit");
}
async function call(url, body) {
  const r = await fetch(origin + "/api/commerce" + url, {
    method: body ? "POST" : "GET",
    headers: { "x-kf-token": token, "content-type": "application/json" },
    body: body ? JSON.stringify(body) : undefined,
  });
  const v = await r.json();
  assert.ok(r.ok, JSON.stringify(v));
  return v;
}
async function until(id, predicate) {
  for (let i = 0; i < 160; i++) {
    const j = await call("/jobs/" + id);
    if (predicate(j)) return j;
    await wait(100);
  }
  throw Error("Job did not reach expected state");
}
const request = {
  kind: "catalog",
  plugin: "generic-article",
  year: 2027,
  args: {
    url: "http://127.0.0.1:18994/catalog",
    school: "接口验收大学",
    majorName: "数据工程",
    college: "计算机学院",
  },
  refresh: true,
};
try {
  await start();
  const first = await call("/jobs", request);
  const result = await until(first.id, (j) =>
    ["completed", "failed"].includes(j.state),
  );
  assert.equal(result.state, "completed", result.error);
  let data = await call("/requirements");
  const scoped = data.records.filter((r) => r.data.school === "接口验收大学");
  assert.equal(scoped.length, 1);
  assert.equal(scoped[0].data.candidate.title, "数据库系统概论");
  assert.ok(data.programs.some((p) => p.major === "数据工程"));
  console.log(
    "ok official fixture -> persisted catalog -> program and book evidence",
  );
  const confirmed = await call("/requirements/" + scoped[0].id + "/confirm", {
    editionChecked: true,
    book: { title: "数据库系统概论", edition: "第5版", author: "王珊" },
  });
  const repeat = await call("/jobs", { ...request, refresh: false });
  assert.equal(
    (await until(repeat.id, (j) => ["completed", "failed"].includes(j.state)))
      .state,
    "completed",
  );
  data = await call("/requirements");
  assert.equal(
    data.records.filter((r) => r.data.school === "接口验收大学").length,
    1,
  );
  assert.equal(
    data.records.find((r) => r.id === scoped[0].id).bookId,
    confirmed.bookId,
  );
  console.log("ok repeat collection preserves book identity and confirmation");
  slow = true;
  const paused = await call("/jobs", request);
  await until(paused.id, (j) => j.state === "running");
  await call("/jobs/" + paused.id + "/pause", {});
  await wait(500);
  assert.equal((await call("/jobs/" + paused.id)).state, "paused");
  await stop();
  await start();
  assert.equal((await call("/jobs/" + paused.id)).state, "paused");
  slow = false;
  await call("/jobs/" + paused.id + "/resume", {});
  assert.equal(
    (await until(paused.id, (j) => ["completed", "failed"].includes(j.state)))
      .state,
    "completed",
  );
  console.log("ok pause, restart, resume with stable results");
  slow = true;
  const interrupted = await call("/jobs", request);
  await until(interrupted.id, (j) => j.state === "running");
  await wait(250);
  await stop();
  await wait(650);
  slow = false;
  await start();
  assert.equal(
    (
      await until(interrupted.id, (j) =>
        ["completed", "failed"].includes(j.state),
      )
    ).state,
    "completed",
  );
  console.log("ok interrupted running job recovers after process restart");
  slow = true;
  const canceled = await call("/jobs", request);
  await until(canceled.id, (j) => j.state === "running");
  await call("/jobs/" + canceled.id + "/cancel", {});
  await wait(500);
  assert.equal((await call("/jobs/" + canceled.id)).state, "canceled");
  console.log("ok running cancellation terminates job");
  slow = false;
  fail = true;
  const broken = await call("/jobs", {
    ...request,
    args: { ...request.args, url: "http://127.0.0.1:18994/failure" },
  });
  assert.equal(
    (await until(broken.id, (j) => ["failed", "completed"].includes(j.state)))
      .state,
    "failed",
  );
  fail = false;
  await call("/jobs/" + broken.id + "/retry", {});
  assert.equal(
    (await until(broken.id, (j) => ["failed", "completed"].includes(j.state)))
      .state,
    "completed",
  );
  console.log("ok failure retained and retry succeeds");
  const library = await call(
    "/library?q=" + encodeURIComponent("数据库") + "&pageSize=1",
  );
  assert.equal(library.total, 1);
  assert.equal(library.items[0].id, confirmed.bookId);
  console.log("ok paginated library and school return same book");
} finally {
  if (server?.exitCode === null) await stop();
  fixture.closeAllConnections();
  await new Promise((r) => fixture.close(r));
  await fs.rm(root, { recursive: true, force: true });
}
