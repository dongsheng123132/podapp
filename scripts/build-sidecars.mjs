/**
 * 把 MCP 桥编译成浮舱的随身程序（Tauri 叫 sidecar）。
 *
 * # 为什么必须有这一步
 *
 * `podapp-mcp` 一直躺在仓库里没进过安装包：`externalBin` 是空的，浮舱代码里
 * 一处都没提到它。也就是说「装一个 `.pod` 就自动成为 MCP 工具」这句话，
 * 到今天为止**只对自己从源码编译的人成立** —— 代码进仓库不等于到用户手上。
 *
 * # 为什么是脚本而不是手动复制一次
 *
 * 手动复制的产物会和源码悄悄错开：改了 MCP 的代码、忘了重新复制，
 * 安装包里还是旧的那个，而构建日志一路绿灯。这和图标那个坑
 * （改了 `icons/` 不 touch `build.rs`，旧图标留在 exe 里）是同一类错。
 * 挂进 `beforeBuildCommand`，让它没法被忘掉。
 *
 * # Tauri 对文件名的要求
 *
 * sidecar 必须叫 `<名字>-<target triple>.exe`，Tauri 打包时按当前 triple 找。
 * 名字对不上的报错是「找不到 external binary」，而人的第一反应是去查路径，
 * 不是去查后缀 —— 所以这里把 triple 从 `rustc -vV` 现读，不写死。
 */

import { execFileSync } from "node:child_process";
import { copyFileSync, mkdirSync, statSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const repo = join(here, "..");
const sidecarDir = join(repo, "apps", "podapp-dock", "src-tauri", "binaries");

/** 当前 target triple。写死会在换机器/换架构时安静地打出一个装不上的包。 */
function hostTriple() {
  const out = execFileSync("rustc", ["-vV"], { encoding: "utf8" });
  const line = out.split("\n").find((l) => l.startsWith("host:"));
  if (!line) throw new Error("rustc -vV 里读不到 host triple");
  return line.slice("host:".length).trim();
}

function run(cmd, args, cwd) {
  execFileSync(cmd, args, { cwd, stdio: "inherit" });
}

const triple = hostTriple();
const exe = process.platform === "win32" ? ".exe" : "";

console.log(`[sidecar] 目标 ${triple}`);
// MCP 桥在**上层 workspace** 里（浮舱刻意不在那个 workspace，见 src-tauri/Cargo.toml
// 的注释），所以这里的 cwd 是仓库根，不是 src-tauri。
run("cargo", ["build", "--release", "-p", "podapp-mcp"], repo);

const built = join(repo, "target", "release", `podapp-mcp${exe}`);
const target = join(sidecarDir, `podapp-mcp-${triple}${exe}`);
mkdirSync(sidecarDir, { recursive: true });
copyFileSync(built, target);

console.log(`[sidecar] ${target} ${(statSync(target).size / 1024).toFixed(0)} KB`);
