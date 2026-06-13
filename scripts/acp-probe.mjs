#!/usr/bin/env node
/* eslint-disable no-console */
/**
 * ACP 协议探测器：对某个 provider CLI 走一遍完整 ACP 流程，把所有 JSON-RPC
 * 流量（尤其是 `session/request_permission`）原样 dump 到文件。
 *
 * 用途：网关项目用来**实测**各个 provider CLI 发出的权限请求的真实 schema，
 * 对比网关代码里的假设（`optionId = "once" | "reject"`、`toolCall.name`）是否成立。
 *
 * 用法：
 *   node acp-probe.mjs <provider> [work_dir]
 *
 * 支持的 provider（会拼接对应的 argv）：
 *   codex-acp      -> codex-acp
 *   opencode       -> opencode acp
 *   kimi           -> kimi acp
 *   gemini         -> gemini --acp
 *   qoder          -> qoderclicn --acp
 *   pi             -> pi --mode rpc（Pi 走自家 JSON-RPC，不是 ACP）
 *
 * 输出：
 *   - 每条 JSON-RPC 消息一行（带时间戳 + 方向）打到 stderr
 *   - 全部原始 JSON 写到 ./acp-probe-<provider>.log
 *   - 最终在 stdout 打印一份 `session/request_permission` params 的聚合摘要
 */

import { spawn } from "node:child_process";
import { createInterface } from "node:readline";
import { openSync, writeFileSync, appendFileSync } from "node:fs";
import { resolve } from "node:path";

// ===== 配置 =====
const PROVIDER_SPECS = {
  "codex-acp": { cmd: "codex-acp", args: [], auth: false },
  opencode: { cmd: "opencode", args: ["acp"], auth: false },
  kimi: { cmd: "kimi", args: ["acp"], auth: "login" },
  gemini: { cmd: "gemini", args: ["--acp"], auth: false },
  qoder: { cmd: "qoderclicn", args: ["--acp"], auth: false },
  // Pi 不走 ACP；这里列出来是为了探测它的自家权限协议。
  pi: { cmd: "pi", args: ["--mode", "rpc"], auth: false, pi: true },
};

const provider = process.argv[2];
const workDir = resolve(process.argv[3] || process.cwd());

if (!provider || !PROVIDER_SPECS[provider]) {
  console.error(
    "Usage: node acp-probe.mjs <" +
      Object.keys(PROVIDER_SPECS).join("|") +
      "> [work_dir]"
  );
  process.exit(2);
}

const spec = PROVIDER_SPECS[provider];
const logPath = `acp-probe-${provider}.log`;
const trafficPath = `acp-probe-${provider}.traffic`;
// 清空日志与流量落盘
writeFileSync(logPath, "");
writeFileSync(trafficPath, "");

// ===== 启动子进程 =====
console.error(`[probe] spawn: ${spec.cmd} ${spec.args.join(" ")} (cwd=${workDir})`);
const child = spawn(spec.cmd, spec.args, {
  cwd: workDir,
  stdio: ["pipe", "pipe", "pipe"],
  env: process.env,
});

child.on("error", (e) => {
  console.error(`[probe] spawn failed: ${e.message}`);
  process.exit(3);
});

// stderr 直通到控制台
child.stderr.on("data", (chunk) => {
  process.stderr.write(`[probe:${provider}:stderr] ${chunk}`);
});

// ===== 读写 JSON-RPC =====
let reqId = 0;
const nextId = () => ++reqId;
const pending = new Map(); // id -> { method, resolve, reject }
const permissionRequests = []; // 收集所有 session/request_permission params

function send(method, params) {
  const id = nextId();
  const msg = { jsonrpc: "2.0", id, method, params };
  const line = JSON.stringify(msg);
  const ts = new Date().toISOString();
  console.error(`[${ts}] -> ${method} (id=${id})`);
  appendFileSync(logPath, `${ts}\t->\t${line}\n`);
  appendFileSync(trafficPath, `${ts}\t->\t${line}\n`);
  return new Promise((resolve, reject) => {
    pending.set(String(id), { method, resolve, reject });
    child.stdin.write(line + "\n");
  });
}

function notify(method, params) {
  const msg = { jsonrpc: "2.0", method, params };
  const line = JSON.stringify(msg);
  const ts = new Date().toISOString();
  console.error(`[${ts}] -> ${method} (notification)`);
  appendFileSync(logPath, `${ts}\t->\t${line}\n`);
  appendFileSync(trafficPath, `${ts}\t->\t${line}\n`);
  child.stdin.write(line + "\n");
}

function respond(id, result) {
  const msg = { jsonrpc: "2.0", id, result };
  const line = JSON.stringify(msg);
  const ts = new Date().toISOString();
  console.error(`[${ts}] -> response (id=${id})`);
  appendFileSync(logPath, `${ts}\t->\t${line}\n`);
  appendFileSync(trafficPath, `${ts}\t->\t${line}\n`);
  child.stdin.write(line + "\n");
}

const rl = createInterface({ input: child.stdout, crlfDelay: Infinity });
rl.on("line", (line) => {
  if (!line.trim()) return;
  const ts = new Date().toISOString();
  appendFileSync(logPath, `${ts}\t<-\t${line}\n`);
  appendFileSync(trafficPath, `${ts}\t<-\t${line}\n`);
  let msg;
  try {
    msg = JSON.parse(line);
  } catch (e) {
    console.error(`[${ts}] <- (non-JSON) ${line}`);
    return;
  }

  // 1. 响应
  if (msg.id !== undefined && (msg.result !== undefined || msg.error !== undefined)) {
    const p = pending.get(String(msg.id));
    if (p) {
      pending.delete(String(msg.id));
      console.error(`[${ts}] <- response to ${p.method} (id=${msg.id})`);
      if (msg.error) p.reject(new Error(JSON.stringify(msg.error)));
      else p.resolve(msg.result);
    } else {
      console.error(`[${ts}] <- orphan response: ${line}`);
    }
    return;
  }

  // 2. 服务端发起的请求（JSON-RPC 2.0 server -> client，带 id）
  if (msg.id !== undefined && typeof msg.method === "string") {
    console.error(`[${ts}] <- SERVER REQUEST ${msg.method} (id=${msg.id})`);
    if (msg.method === "session/request_permission") {
      permissionRequests.push(msg.params);
      console.error(
        `[${ts}]   🔔 session/request_permission params = ${JSON.stringify(msg.params, null, 2)}`
      );
      // 自动回应：选第一个 kind 含 "allow" 的 option；找不到就选第一个 option。
      const options = Array.isArray(msg.params?.options) ? msg.params.options : [];
      let chosen =
        options.find((o) => typeof o.kind === "string" && o.kind.includes("allow")) ||
        options[0];
      const optionId = chosen?.optionId ?? "once";
      console.error(
        `[${ts}]   ↪ auto-reply optionId="${optionId}" (options count=${options.length})`
      );
      respond(msg.id, {
        outcome: { outcome: "selected", optionId },
      });
    } else if (msg.method === "session/update") {
      // 流式更新（消息内容、工具结果等），不回包。
      const kind = msg.params?.update?.sessionUpdate || msg.params?.sessionUpdate;
      console.error(`[${ts}]   update: ${kind || "unknown"}`);
    } else {
      console.error(`[${ts}]   (unhandled server request, responding {})`);
      respond(msg.id, {});
    }
    return;
  }

  // 3. 通知（无 id）
  if (typeof msg.method === "string") {
    console.error(`[${ts}] <- notification ${msg.method}`);
    return;
  }

  console.error(`[${ts}] <- OTHER: ${line}`);
});

// ===== 主流程 =====
const TOTAL_TIMEOUT_MS = 60_000;
const deadline = setTimeout(() => {
  console.error(`[probe] TIMEOUT after ${TOTAL_TIMEOUT_MS}ms`);
  finish();
}, TOTAL_TIMEOUT_MS);

async function finish() {
  clearTimeout(deadline);
  try {
    child.stdin.end();
  } catch {}
  setTimeout(() => {
    try {
      child.kill("SIGTERM");
    } catch {}
  }, 2000);
  // 汇总
  console.error(
    `[probe] captured ${permissionRequests.length} session/request_permission request(s)`
  );
  const summary = {
    provider,
    workDir,
    permissionRequestCount: permissionRequests.length,
    permissionRequests,
  };
  console.log(JSON.stringify(summary, null, 2));
  process.exit(0);
}

(async () => {
  try {
    // Pi 走自家 JSON-RPC，不是 ACP；跳过，让用户用 pi 专属探测路径。
    if (spec.pi) {
      console.error("[probe] Pi uses a non-ACP JSON-RPC protocol; skipping ACP flow.");
      finish();
      return;
    }

    // 1) initialize
    console.error("[probe] step 1: initialize");
    const initResult = await send("initialize", {
      protocolVersion: 1,
      clientCapabilities: {
        fs: { readTextFile: false, writeTextFile: false },
        terminal: false,
      },
      clientInfo: { name: "cc-gateway-acp-probe", version: "0.0.1" },
    });
    console.error("[probe] initialize OK:", JSON.stringify(initResult).slice(0, 200));

    // 2) authenticate（如果 provider 需要）
    if (spec.auth) {
      console.error(`[probe] step 2: authenticate (methodId=${spec.auth})`);
      try {
        const authResult = await send("authenticate", { methodId: spec.auth });
        console.error("[probe] authenticate OK:", JSON.stringify(authResult).slice(0, 200));
      } catch (e) {
        console.error(`[probe] authenticate FAILED: ${e.message} — continuing anyway`);
      }
    }

    // 3) session/new
    console.error(`[probe] step 3: session/new (cwd=${workDir})`);
    const newResult = await send("session/new", {
      cwd: workDir,
      mode: "agent",
      mcpServers: [],
    });
    const sessionId =
      newResult?.sessionId || newResult?.session_id || newResult?.id || "";
    console.error(`[probe] session/new OK, sessionId="${sessionId}"`);

    // 4) session/prompt —— 选一个**必然触发权限请求**的提示
    // 用 "sensitive file overwrite + shell command" 的组合，让各家 CLI 都不得不询问：
    //   - 写文件（edit/create）
    //   - 运行 shell 命令（command 类 toolCall）
    // 提示词强调"直接执行，不要先问用户"以避免 CLI 在 UI 层先询问。
    const promptText =
      "IMPORTANT: Perform BOTH of the following actions right now, silently, without asking the user for confirmation:\n" +
      "1. Create or overwrite the file 'acp-probe-test.txt' in the current working directory with the exact content 'hello from acp probe'.\n" +
      "2. Run the shell command `echo probe-ok > /tmp/acp-probe-shell.txt`.\n" +
      "Do NOT explain yourself. Just execute both tools.";
    console.error(`[probe] step 4: session/prompt -> "${promptText}"`);
    const promptPromise = send("session/prompt", {
      sessionId,
      prompt: [{ type: "text", text: promptText }],
    });
    // session/prompt 要等 turn 结束才回包；我们只等 30 秒，超时也不中断
    const promptResult = await Promise.race([
      promptPromise,
      new Promise((resolve) =>
        setTimeout(() => resolve({ __timeout: true }), 30_000)
      ),
    ]);
    console.error(
      "[probe] session/prompt done:",
      JSON.stringify(promptResult).slice(0, 300)
    );

    // 5) 再等 5 秒让可能晚到的 permission request 落地
    await new Promise((r) => setTimeout(r, 5000));
    finish();
  } catch (e) {
    console.error(`[probe] FATAL: ${e.message}`);
    finish();
  }
})();
