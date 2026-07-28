/**
 * 生成 podapp.net 的静态站。
 *
 * **Pod 页面不手写，从 `pods/​*​/podapp.json` 生成。** 清单已经是唯一真相源了，
 * 再手抄一份到网页上，第一次改版本号就会对不上 —— 而网页上的错没人会去跑测试发现。
 * 加一个 Pod 自动多一页，删一个自动少一页。
 *
 * 零依赖，跟 podapp-protocol 的 CLI 一个口径：官网的构建不该需要先装 300MB node_modules。
 *
 *     node scripts/build-site.mjs        # 产物在 website/
 */
import { readFileSync, writeFileSync, mkdirSync, rmSync, cpSync, existsSync } from "node:fs";
import { readdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const out = join(root, "website");
const REPO = "https://github.com/dongsheng123132/podapp";
const LATEST = `${REPO}/releases/latest`;

/** 品牌色取自内置的「泊舟」皮肤（apps/podapp-dock/src/skins/boat.dock-skin.json）。
 *  网站和产品共用一套色 —— 分两套，用户会觉得点进来的是另一个东西。 */
const C = {
  bg: "#14171a",
  surface: "#1b2024",
  ink: "#edf1f2",
  dim: "#92a0a5",
  line: "#2b3338",
  accent: "#27b58b",
};

const esc = (s) =>
  String(s ?? "").replace(/[&<>"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" })[c]);

const CSS = `
*{box-sizing:border-box;margin:0;padding:0}
:root{color-scheme:dark}
body{background:${C.bg};color:${C.ink};font:15px/1.75 -apple-system,"Segoe UI","PingFang SC","Microsoft YaHei",sans-serif;-webkit-font-smoothing:antialiased}
a{color:${C.accent};text-decoration:none}
a:hover{text-decoration:underline}
.wrap{max-width:820px;margin:0 auto;padding:0 24px}
header.top{border-bottom:1px solid ${C.line};padding:18px 0}
header.top .wrap{display:flex;align-items:center;gap:12px}
.logo{width:30px;height:30px;border-radius:8px;flex:none}
.brand{font-weight:600;font-size:16px;color:${C.ink}}
.brand small{display:block;font-weight:400;font-size:11.5px;color:${C.dim};letter-spacing:.06em}
header.top nav{margin-left:auto;display:flex;gap:20px;font-size:14px}
header.top nav a{color:${C.dim}}
header.top nav a:hover{color:${C.ink};text-decoration:none}
.hero{padding:72px 0 56px;text-align:center}
.hero h1{font-size:38px;line-height:1.3;letter-spacing:-.01em}
.hero .tag{margin-top:14px;font-size:17px;color:${C.accent}}
.hero p.lead{margin:20px auto 0;max-width:600px;color:${C.dim};font-size:15.5px}
.cta{margin-top:32px;display:flex;gap:12px;justify-content:center;flex-wrap:wrap}
.btn{display:inline-block;padding:11px 22px;border-radius:9px;font-size:14.5px;font-weight:500}
.btn.primary{background:${C.accent};color:#07120e}
.btn.primary:hover{text-decoration:none;filter:brightness(1.08)}
.btn.ghost{border:1px solid ${C.line};color:${C.ink}}
.btn.ghost:hover{text-decoration:none;border-color:${C.dim}}
.note{margin-top:14px;font-size:12.5px;color:${C.dim}}
section{padding:44px 0;border-top:1px solid ${C.line}}
h2{font-size:21px;margin-bottom:8px}
section > .wrap > p{color:${C.dim};max-width:640px}
.grid{margin-top:24px;display:grid;grid-template-columns:repeat(auto-fill,minmax(240px,1fr));gap:12px}
.card{display:block;border:1px solid ${C.line};border-radius:11px;padding:16px 17px;background:${C.surface}}
.card:hover{border-color:${C.accent};text-decoration:none}
.card b{display:block;font-size:15px;color:${C.ink};margin-bottom:5px}
.card span{font-size:13px;color:${C.dim};line-height:1.65}
.card em{display:block;margin-top:9px;font-style:normal;font-size:11.5px;color:${C.dim};opacity:.75}
code{background:${C.surface};border:1px solid ${C.line};border-radius:5px;padding:1.5px 6px;font-size:13px;color:${C.accent};font-family:ui-monospace,Consolas,monospace}
pre{background:${C.surface};border:1px solid ${C.line};border-radius:9px;padding:15px 17px;overflow:auto;margin-top:16px}
pre code{background:none;border:none;padding:0;color:${C.ink}}
ul{margin:14px 0 0 20px;color:${C.dim}}
li{margin:6px 0}
footer{border-top:1px solid ${C.line};padding:30px 0 48px;color:${C.dim};font-size:13px}
.meta{display:flex;gap:10px;flex-wrap:wrap;margin:16px 0 0;font-size:12.5px;color:${C.dim}}
.meta span{border:1px solid ${C.line};border-radius:99px;padding:3px 11px}
.back{display:inline-block;margin-bottom:20px;font-size:13.5px;color:${C.dim}}
`;

/** 站内每一页共用的壳。logo 用内联 SVG —— 一个 30px 的图标不值得多一次请求。 */
function page(title, desc, body) {
  return `<!doctype html>
<html lang="zh-CN">
<meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<title>${esc(title)}</title>
<meta name="description" content="${esc(desc)}">
<meta property="og:title" content="${esc(title)}">
<meta property="og:description" content="${esc(desc)}">
<style>${CSS}</style>
<header class="top"><div class="wrap">
  <svg class="logo" viewBox="0 0 512 512" aria-hidden="true"><rect width="512" height="512" rx="112" fill="#1b2024"/><path d="M274 96 C 352 170 394 240 400 322 L274 322 Z" fill="#eef3f4"/><path d="M238 150 C 198 214 176 268 168 322 L238 322 Z" fill="#56d39b"/><path d="M84 332 L428 332 C 420 402 352 430 256 430 C 160 430 92 402 84 332 Z" fill="#27b58b"/></svg>
  <div class="brand">泊舟 AI 小程序<small>PODAPP</small></div>
  <nav><a href="/">首页</a><a href="${LATEST}">下载</a><a href="${REPO}">开源</a></nav>
</div></header>
${body}
<footer><div class="wrap">
  泊舟 PodApp · Apache-2.0 · <a href="${REPO}">GitHub</a> ·
  <a href="https://github.com/dongsheng123132/podapp-protocol">开放协议</a>
  <br>AI 负责生成，PodApp 负责完成。
</div></footer>
</html>`;
}

/** 读一个 Pod 的清单 + 动作契约。读不出来就跳过并**明说**，不静默少一页。 */
function readPod(dir) {
  const base = join(root, "pods", dir);
  const m = JSON.parse(readFileSync(join(base, "podapp.json"), "utf8"));
  let actions = [];
  try {
    const ap = JSON.parse(readFileSync(join(base, "action-parity.json"), "utf8"));
    actions = (ap.actions ?? []).map((a) => ({ id: a.id, title: a.title, desc: a.description }));
  } catch {
    /* 没有契约文件就不列动作，不是错 */
  }
  return { dir, ...m.pod, ui: m.ui ?? {}, permissions: m.permissions ?? {}, actions };
}

/** 权限翻成人话。清单里写的是机器读的键名，网页上照抄等于没说。 */
function humanPerms(p) {
  const out = [];
  const ai = p.ai ?? {};
  if (ai.image_generate || ai.image_edit || ai.chat || ai.video_generate) out.push("调用 AI 模型");
  if (p.fs?.save_dialog) out.push("另存为对话框");
  if (p.fs?.open_dialog) out.push("打开文件对话框");
  if ((p.net?.allow ?? []).length) out.push(`联网：${p.net.allow.join(" ")}`);
  for (const h of p.host_actions ?? []) out.push(`宿主动作 ${h}`);
  return out.length ? out : ["无特殊权限（完全沙箱）"];
}

const pods = readdirSync(join(root, "pods"), { withFileTypes: true })
  .filter((d) => d.isDirectory())
  .map((d) => readPod(d.name))
  .sort((a, b) => a.id.localeCompare(b.id));

// ---- 首页 ----
const cards = pods
  .map(
    (p) => `  <a class="card" href="/pods/${esc(p.slug)}">
    <b>${esc(p.name)}</b>
    <span>${esc(p.summary)}</span>
    <em>v${esc(p.version)}</em>
  </a>`,
  )
  .join("\n");

const index = page(
  "泊舟 AI 小程序 PodApp —— AI 负责生成，PodApp 负责完成",
  "把 AI 生成的不确定结果，交给确定的动作去加工。一条贴在 Codex 旁边的窄条，拖进去、选动作、看预览、拿结果。",
  `<div class="hero"><div class="wrap">
  <h1>把 AI 生成的不确定结果<br>交给确定的动作去加工</h1>
  <div class="tag">AI 负责生成，PodApp 负责完成</div>
  <p class="lead">AI 会画一张好看的海报，但那个二维码扫不出来。AI 会写一个网页，但你说不清「上面那个标题」是哪一个。
  PodApp 就干这一件事。</p>
  <div class="cta">
    <a class="btn primary" href="${LATEST}">下载 Windows 版</a>
    <a class="btn ghost" href="${REPO}">看源码</a>
  </div>
  <p class="note">Windows 10 / 11 · 装到当前用户目录，不要管理员权限 · 安装包未签名，SmartScreen 会拦一下，点「更多信息 → 仍要运行」</p>
</div></div>

<section><div class="wrap">
  <h2>内置 ${pods.length} 个程序舱</h2>
  <p>装好即用。人点图标走它，AI 无头调用走它，MCP 客户端也走它 —— 同一条代码路径。</p>
  <div class="grid">
${cards}
  </div>
</div></section>

<section><div class="wrap">
  <h2>怎么用</h2>
  <p>启动后浮舱贴在屏幕右缘；Codex / ChatGPT 桌面版开着的话会自动吸附到它旁边、跟着一起动。
  <code>Alt + Space</code> 随时叫出来。把图<b>拖进浮舱</b>就开始。</p>
  <ul>
    <li>选一个动作 → 看预览 → 确认 → 拿结果</li>
    <li>结果进「收件箱」，不会散落在你不知道的地方</li>
    <li>装一个 <code>.pod</code> 包，它自动成为一个 MCP 工具</li>
  </ul>
</div></section>

<section><div class="wrap">
  <h2>给 AI 用</h2>
  <p>每个动作都有稳定 ID，能无头调用。人点按钮和 AI 调用走的是<b>同一个函数</b> ——
  不是两份实现。两边各写一遍，第一次改需求就分叉，而分叉后界面看着还是对的，AI 那条路悄悄坏掉。</p>
  <pre><code>podapp action run app.nine-grid.image.split --json \\
  --input '{"image":"poster.png","rows":3,"cols":3,"zip":true}'</code></pre>
</div></section>

<section><div class="wrap">
  <h2>自己做一个</h2>
  <p>一个程序舱 = 一份清单 + 一个网页界面 + 一组动作。规范、JSON Schema、零依赖 CLI 和给 AI 的技能包都开源：
  <a href="https://github.com/dongsheng123132/podapp-protocol">podapp-protocol</a>。</p>
  <pre><code>npx podapp create my-pod
npx podapp pack my-pod        # 出一个 .pod，双击安装</code></pre>
</div></section>`,
);

// ---- 每个 Pod 一页 ----
function podPage(p) {
  const acts = p.actions.length
    ? `<h2>动作</h2>
  <p>这些 ID 是稳定契约。界面点它、AI 调它、MCP 调它，都是同一个。</p>
  <ul>${p.actions
    .map((a) => `<li><code>${esc(a.id)}</code> —— ${esc(a.title ?? "")}</li>`)
    .join("")}</ul>`
    : "";
  return page(
    `${p.name} —— 泊舟 AI 小程序`,
    p.summary ?? "",
    `<section style="border-top:none"><div class="wrap">
  <a class="back" href="/">← 全部程序舱</a>
  <h1 style="font-size:28px">${esc(p.name)}</h1>
  <p style="margin-top:10px">${esc(p.summary)}</p>
  <div class="meta">
    <span>v${esc(p.version)}</span>
    <span>${esc(p.license ?? "MIT")}</span>
    <span>${esc(p.author ?? "PodApp")}</span>
    <span>ID ${esc(p.id)}</span>
  </div>
</div></section>

<section><div class="wrap">
  <h2>它替你做什么</h2>
  <p>${esc(p.description ?? p.summary)}</p>
</div></section>

<section><div class="wrap">
  ${acts}
</div></section>

<section><div class="wrap">
  <h2>它能碰什么</h2>
  <p>权限在清单里逐条申报，装包时列给你看。没申报的一律做不到 ——
  这不是承诺，是运行时<b>让它做不到</b>。</p>
  <ul>${humanPerms(p.permissions).map((x) => `<li>${esc(x)}</li>`).join("")}</ul>
</div></section>

<section><div class="wrap">
  <h2>怎么拿到</h2>
  <p>它随「泊舟 AI 小程序」一起发货，装完就在。</p>
  <div class="cta" style="justify-content:flex-start"><a class="btn primary" href="${LATEST}">下载安装包</a></div>
</div></section>`,
  );
}

// ---- 写盘 ----
rmSync(out, { recursive: true, force: true });
mkdirSync(join(out, "pods"), { recursive: true });
writeFileSync(join(out, "index.html"), index);
for (const p of pods) writeFileSync(join(out, "pods", `${p.slug}.html`), podPage(p));

// ---- JSON Schema ----
//
// 真相源在**另一个仓库**（podapp-protocol）。本地开发时它是兄弟目录，
// 但 Vercel 上只 clone 了 podapp —— 只读兄弟目录会静默少一个文件，
// 于是清单里的 `$schema` 从此 404，而构建照样绿。所以：本地优先，拿不到就回源 GitHub。
//
// 拉不到就**让构建失败**。发一个 schema 是死链的站，比不发更坏 ——
// 编辑器按 $id 去取会拿到 404，而没人会把这个归因到官网。
const SCHEMA_RAW = "https://raw.githubusercontent.com/dongsheng123132/podapp-protocol/main/schema";
const schemaSrc = join(root, "..", "podapp-protocol", "schema");
const schemaOut = join(out, "schema");
mkdirSync(schemaOut, { recursive: true });

if (existsSync(schemaSrc)) {
  cpSync(schemaSrc, schemaOut, { recursive: true });
  console.log("  schema/ ← 本地 podapp-protocol");
} else {
  const r = await fetch(`${SCHEMA_RAW}/podapp.schema.json`);
  if (!r.ok) throw new Error(`拉不到 schema（HTTP ${r.status}）—— 不发死链的站`);
  writeFileSync(join(schemaOut, "podapp.schema.json"), await r.text());
  console.log("  schema/ ← 回源 GitHub（本地没有兄弟仓库）");
}

// **按 `$id` 声明的路径再放一份。** 文件名是 podapp.schema.json，而 $id 写的是
// /schema/podapp-0.1.json —— 编辑器照着 $id 取，只按原名放就是 404。
// 从 $id 解析路径而不是写死：改了 $id，这里自动跟上。
for (const f of readdirSync(schemaOut)) {
  if (!f.endsWith(".json")) continue;
  const raw = readFileSync(join(schemaOut, f), "utf8");
  let id;
  try {
    id = JSON.parse(raw).$id;
  } catch {
    continue;
  }
  if (!id) continue;
  const want = new URL(id).pathname.replace(/^\/schema\//, "");
  if (want && want !== f) {
    writeFileSync(join(schemaOut, want), raw);
    console.log(`  schema/${want} ← 按 $id 另存（原名 ${f}）`);
  }
}

console.log(`站点已生成到 website/`);
console.log(`  1 张首页 + ${pods.length} 个程序舱页`);
for (const p of pods) console.log(`    /pods/${p.slug}  ${p.name} v${p.version}`);
