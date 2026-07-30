/**
 * 生成 podapp.net 的静态站（中 / 英双语）。
 *
 * **Pod 内容不手写，从 `pods/​*​/podapp.json` 生成。** 清单已经是唯一真相源，
 * 再手抄一份到网页上，第一次改版本号就会对不上 —— 而网页上的错没人会去跑测试发现。
 * 英文文案也在清单里（顶层 `i18n.en`），中英同源；`pod` 段是 additionalProperties:false，
 * 塞不进去，顶层是 true，运行时实测接受。
 *
 * **整站零 JavaScript** —— 所以 CSP 里 script-src 能锁成 'none'。
 * 语言切换是两套静态页 + 链接，不是运行时判断：没有 JS 就没有闪烁，也不需要 cookie。
 *
 * 零依赖，跟 podapp-protocol 的 CLI 一个口径。
 *
 *     node scripts/build-site.mjs        # 产物在 website/
 */
import { readFileSync, writeFileSync, mkdirSync, rmSync, cpSync, existsSync, readdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const out = join(root, "website");
const REPO = "https://github.com/dongsheng123132/podapp";
const PROTO = "https://github.com/dongsheng123132/podapp-protocol";
const LATEST = `${REPO}/releases/latest`;
const SITE = "https://podapp.net";

/** 品牌色取自内置的「泊舟」皮肤（apps/podapp-dock/src/skins/boat.dock-skin.json）。
 *  网站和产品共用一套色 —— 分两套，用户会觉得点进来的是另一个东西。 */
const C = {
  bg: "#14171a",
  surface: "#1b2024",
  raise: "#20262b",
  ink: "#edf1f2",
  dim: "#92a0a5",
  faint: "#6b787d",
  line: "#2b3338",
  accent: "#27b58b",
  accent2: "#56d39b",
};

const esc = (s) =>
  String(s ?? "").replace(/[&<>"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" })[c]);

/** 仓库 Markdown 是宣言的唯一真相源。官网只渲染，不再手抄一份。 */
const DOC_LINKS = {
  "./MANIFESTO.md": "/en/manifesto",
  "./MANIFESTO.zh-CN.md": "/manifesto",
  "./PRINCIPLES.md": `${REPO}/blob/main/PRINCIPLES.md`,
  "./PRINCIPLES.zh-CN.md": `${REPO}/blob/main/PRINCIPLES.zh-CN.md`,
  "./SIGNATORIES.md": `${REPO}/blob/main/SIGNATORIES.md`,
};

function inlineMarkdown(s) {
  return esc(s)
    .replace(/`([^`]+)`/g, "<code>$1</code>")
    .replace(/\*\*([^*]+)\*\*/g, "<strong>$1</strong>")
    .replace(/\[([^\]]+)\]\(([^)]+)\)/g, (_, label, href) => {
      const target = DOC_LINKS[href] ?? href;
      return `<a href="${esc(target)}">${label}</a>`;
    });
}

/**
 * 只实现宣言实际使用的 Markdown 子集。输入是仓库内受信文件，输出仍先逐段转义；
 * 支持标题、段落、引用、无序列表和分隔线。保持零依赖，也避免官网与 Markdown 漂移。
 */
function renderMarkdown(source) {
  const lines = source.replace(/\r\n/g, "\n").split("\n");
  const out = [];
  let paragraph = [];
  let listOpen = false;

  const flushParagraph = () => {
    if (!paragraph.length) return;
    out.push(`<p>${inlineMarkdown(paragraph.join(" "))}</p>`);
    paragraph = [];
  };
  const closeList = () => {
    if (!listOpen) return;
    out.push("</ul>");
    listOpen = false;
  };

  for (const raw of lines) {
    const line = raw.trim();
    if (!line) {
      flushParagraph();
      closeList();
      continue;
    }

    const heading = /^(#{1,3})\s+(.+)$/.exec(line);
    if (heading) {
      flushParagraph();
      closeList();
      const level = heading[1].length;
      out.push(`<h${level}>${inlineMarkdown(heading[2])}</h${level}>`);
      continue;
    }

    if (/^---+$/.test(line)) {
      flushParagraph();
      closeList();
      out.push("<hr>");
      continue;
    }

    if (line.startsWith("> ")) {
      flushParagraph();
      closeList();
      out.push(`<blockquote>${inlineMarkdown(line.slice(2))}</blockquote>`);
      continue;
    }

    if (line.startsWith("- ")) {
      flushParagraph();
      if (!listOpen) {
        out.push("<ul>");
        listOpen = true;
      }
      out.push(`<li>${inlineMarkdown(line.slice(2))}</li>`);
      continue;
    }

    closeList();
    paragraph.push(line);
  }

  flushParagraph();
  closeList();
  return out.join("\n");
}

/** 界面文案。Pod 的内容来自清单，这里只有站本身的壳。 */
const T = {
  zh: {
    htmlLang: "zh-CN",
    other: "English",
    otherHref: (p) => `/en${p === "/" ? "" : p}`,
    nav: { home: "首页", manifesto: "宣言", download: "下载", source: "开源" },
    brandSub: "泊舟 AI 小程序",
    heroTitle: "把 AI 生成的不确定结果<br>交给确定的动作去加工",
    tagline: "AI 负责生成，PodApp 负责完成",
    lead: "AI 会画一张好看的海报，但那个二维码扫不出来。AI 会写一个网页，但你说不清「上面那个标题」是哪一个。PodApp 就干这一件事。",
    ctaDownload: "下载 Windows 版",
    ctaManifesto: "阅读 AI GUI 宣言",
    note: "Windows 10 / 11 · 装到当前用户目录，不要管理员权限 · 安装包未签名，SmartScreen 会拦一下，点「更多信息 → 仍要运行」",
    flow: ["拖进去", "选一个动作", "看预览", "确认", "拿结果"],
    podsTitle: (n) => `内置 ${n} 个程序舱`,
    podsLead: "装好即用。人点图标走它，AI 无头调用走它，MCP 客户端也走它 —— 同一条代码路径。",
    howTitle: "怎么用",
    howLead:
      '启动后浮舱贴在屏幕右缘；Codex / ChatGPT 桌面版开着的话会自动吸附到它旁边、跟着一起动。<code>Alt + Space</code> 随时叫出来。把图<b>拖进浮舱</b>就开始。',
    howList: [
      "结果进「收件箱」，不会散落在你不知道的地方",
      "装一个 <code>.pod</code> 包，它自动成为一个 MCP 工具",
      "浮舱自己检查更新，不用你再来下载一次",
    ],
    aiTitle: "给 AI 用",
    aiLead:
      "每个动作都有稳定 ID，能无头调用。人点按钮和 AI 调用走的是<b>同一个函数</b> —— 不是两份实现。两边各写一遍，第一次改需求就分叉，而分叉后界面看着还是对的，AI 那条路悄悄坏掉。",
    makeTitle: "自己做一个",
    makeLead: `一个程序舱 = 一份清单 + 一个网页界面 + 一组动作。规范、JSON Schema、零依赖 CLI 和给 AI 的技能包都开源：<a href="${PROTO}">podapp-protocol</a>。`,
    backAll: "← 全部程序舱",
    doesTitle: "它替你做什么",
    actionsTitle: "动作",
    actionsLead: "这些 ID 是稳定契约。界面点它、AI 调它、MCP 调它，都是同一个。",
    permsTitle: "它能碰什么",
    permsLead:
      "权限在清单里逐条申报，装包时列给你看。没申报的一律做不到 —— 这不是承诺，是运行时<b>让它做不到</b>。",
    getTitle: "怎么拿到",
    getLead: "它随「泊舟 AI 小程序」一起发货，装完就在。",
    getBtn: "下载安装包",
    noPerm: "无特殊权限（完全沙箱）",
    permAi: "调用 AI 模型",
    permSave: "另存为对话框",
    permOpen: "打开文件对话框",
    permNet: (v) => `联网：${v}`,
    permHost: (v) => `宿主动作 ${v}`,
    footer: "AI 负责生成，PodApp 负责完成。",
    specLink: "开放协议",
  },
  en: {
    htmlLang: "en",
    other: "简体中文",
    otherHref: (p) => (p === "/" ? "/" : p),
    nav: { home: "Home", manifesto: "Manifesto", download: "Download", source: "Source" },
    brandSub: "PodApp",
    heroTitle: "Hand AI's uncertain output<br>to actions that are certain",
    tagline: "AI generates. PodApp finishes.",
    lead: "AI draws a beautiful poster, but the QR code scans into nothing. AI writes a page, but you can't describe which heading you mean. That's the one thing PodApp does.",
    ctaDownload: "Download for Windows",
    ctaManifesto: "Read the AI GUI Manifesto",
    note: "Windows 10 / 11 · Installs per-user, no admin rights · Unsigned installer — SmartScreen will warn; click “More info → Run anyway”",
    flow: ["Drop it in", "Pick an action", "See the preview", "Confirm", "Take the result"],
    podsTitle: (n) => `${n} pods included`,
    podsLead:
      "Ready the moment you install. Clicking the icon, calling it headless from AI, and calling it over MCP all run the same code path.",
    howTitle: "How it works",
    howLead:
      'The dock clings to the right edge of your screen; if the Codex / ChatGPT desktop app is open it snaps beside that window and follows it. <code>Alt + Space</code> brings it up anywhere. <b>Drop an image on it</b> to start.',
    howList: [
      "Results land in an inbox — not scattered somewhere you have to hunt for",
      "Install one <code>.pod</code> and it becomes an MCP tool automatically",
      "The dock updates itself; you don't come back here to re-download",
    ],
    aiTitle: "Built for AI callers",
    aiLead:
      "Every action has a stable ID and runs headless. The button a human clicks and the call an agent makes go through <b>the same function</b> — not two implementations. Write it twice and the first requirement change forks them, and after the fork the UI still looks right while the AI path quietly breaks.",
    makeTitle: "Build your own",
    makeLead: `A pod is a manifest, a web UI, and a set of actions. The spec, JSON Schema, zero-dependency CLI and the AI skill pack are all open source: <a href="${PROTO}">podapp-protocol</a>.`,
    backAll: "← All pods",
    doesTitle: "What it does for you",
    actionsTitle: "Actions",
    actionsLead: "These IDs are a stable contract. The UI, an AI agent and an MCP client all call the same one.",
    permsTitle: "What it can touch",
    permsLead:
      "Permissions are declared one by one in the manifest and shown to you at install time. Anything undeclared simply cannot happen — that's not a promise, the runtime <b>makes it impossible</b>.",
    getTitle: "How to get it",
    getLead: "It ships inside PodApp — install once and it's there.",
    getBtn: "Download installer",
    noPerm: "No special permissions (fully sandboxed)",
    permAi: "Calls AI models",
    permSave: "Save-file dialog",
    permOpen: "Open-file dialog",
    permNet: (v) => `Network: ${v}`,
    permHost: (v) => `Host action ${v}`,
    footer: "AI generates. PodApp finishes.",
    specLink: "Open protocol",
  },
};

const CSS = `
*{box-sizing:border-box;margin:0;padding:0}
:root{color-scheme:dark}
body{background:${C.bg};color:${C.ink};font:15px/1.75 -apple-system,"Segoe UI","PingFang SC","Microsoft YaHei",sans-serif;-webkit-font-smoothing:antialiased}
a{color:${C.accent};text-decoration:none}
a:hover{text-decoration:underline}
.wrap{max-width:840px;margin:0 auto;padding:0 24px}

header.top{position:sticky;top:0;z-index:9;background:${C.bg}f2;backdrop-filter:blur(8px);border-bottom:1px solid ${C.line};padding:15px 0}
header.top .wrap{display:flex;align-items:center;gap:12px}
.logo{width:30px;height:30px;border-radius:8px;flex:none}
.brand{font-weight:600;font-size:15.5px;color:${C.ink};line-height:1.25}
.brand small{display:block;font-weight:400;font-size:11px;color:${C.faint};letter-spacing:.09em;text-transform:uppercase}
header.top nav{margin-left:auto;display:flex;align-items:center;gap:18px;font-size:13.5px}
header.top nav a{color:${C.dim}}
header.top nav a:hover{color:${C.ink};text-decoration:none}
.lang{border:1px solid ${C.line};border-radius:99px;padding:4px 12px;color:${C.dim} !important;font-size:12.5px}
.lang:hover{border-color:${C.accent};color:${C.accent} !important}

.hero{padding:82px 0 60px;text-align:center;background:radial-gradient(760px 320px at 50% -60px, rgba(39,181,139,.13), transparent 70%)}
.hero h1{font-size:40px;line-height:1.28;letter-spacing:-.015em;font-weight:650}
.hero .tag{margin-top:16px;font-size:16.5px;color:${C.accent};font-weight:500}
.hero p.lead{margin:20px auto 0;max-width:600px;color:${C.dim};font-size:15.5px}
.cta{margin-top:34px;display:flex;gap:12px;justify-content:center;flex-wrap:wrap}
.btn{display:inline-block;padding:12px 24px;border-radius:10px;font-size:14.5px;font-weight:500;transition:.15s}
.btn.primary{background:${C.accent};color:#06120d}
.btn.primary:hover{text-decoration:none;background:${C.accent2};transform:translateY(-1px)}
.btn.ghost{border:1px solid ${C.line};color:${C.ink}}
.btn.ghost:hover{text-decoration:none;border-color:${C.dim}}
.note{margin-top:16px;font-size:12.5px;color:${C.faint};max-width:620px;margin-left:auto;margin-right:auto}

/* 流程条：纯 CSS，没有 JS。箭头用伪元素，窄屏自动换行 */
.flow{margin:38px auto 0;display:flex;justify-content:center;align-items:center;gap:8px;flex-wrap:wrap;font-size:13px;color:${C.dim}}
.flow i{font-style:normal;border:1px solid ${C.line};background:${C.surface};border-radius:99px;padding:6px 15px}
.flow i:not(:last-child)::after{content:"→";margin-left:13px;color:${C.faint}}

section{padding:52px 0;border-top:1px solid ${C.line}}
h2{font-size:22px;margin-bottom:9px;letter-spacing:-.01em}
section > .wrap > p{color:${C.dim};max-width:660px}
.grid{margin-top:26px;display:grid;grid-template-columns:repeat(auto-fill,minmax(245px,1fr));gap:12px}
.card{display:block;border:1px solid ${C.line};border-radius:12px;padding:17px 18px;background:${C.surface};transition:.15s}
.card:hover{border-color:${C.accent};background:${C.raise};text-decoration:none;transform:translateY(-2px)}
.card b{display:block;font-size:15px;color:${C.ink};margin-bottom:6px}
.card span{font-size:13px;color:${C.dim};line-height:1.65}
.card em{display:block;margin-top:10px;font-style:normal;font-size:11.5px;color:${C.faint}}
code{background:${C.surface};border:1px solid ${C.line};border-radius:5px;padding:1.5px 6px;font-size:13px;color:${C.accent2};font-family:ui-monospace,Consolas,monospace}
pre{background:${C.surface};border:1px solid ${C.line};border-radius:10px;padding:16px 18px;overflow:auto;margin-top:18px}
pre code{background:none;border:none;padding:0;color:${C.ink};font-size:12.5px;line-height:1.7}
ul{margin:15px 0 0 20px;color:${C.dim}}
li{margin:7px 0}
footer{border-top:1px solid ${C.line};padding:32px 0 52px;color:${C.faint};font-size:13px}
.meta{display:flex;gap:9px;flex-wrap:wrap;margin:18px 0 0;font-size:12.5px;color:${C.dim}}
.meta span{border:1px solid ${C.line};border-radius:99px;padding:3px 12px}
.back{display:inline-block;margin-bottom:22px;font-size:13.5px;color:${C.dim}}
.manifesto{max-width:760px;padding-top:58px;padding-bottom:76px}
.manifesto h1{font-size:38px;line-height:1.24;letter-spacing:-.02em;margin-bottom:12px}
.manifesto h2{font-size:24px;line-height:1.35;margin:42px 0 14px}
.manifesto h3{font-size:18px;line-height:1.45;margin:30px 0 10px}
.manifesto p{color:${C.dim};margin:13px 0;max-width:720px}
.manifesto strong{color:${C.ink}}
.manifesto blockquote{margin:24px 0;padding:16px 20px;border-left:3px solid ${C.accent};background:${C.surface};color:${C.ink};font-size:17px}
.manifesto ul{margin:16px 0 22px 24px}
.manifesto hr{border:0;border-top:1px solid ${C.line};margin:42px 0}
@media(max-width:640px){
  header.top nav{gap:10px}
  header.top nav .nav-optional{display:none}
  .hero{padding:56px 0 44px}
  .hero h1{font-size:29px}
  .manifesto h1{font-size:31px}
}
`;

const LOGO = `<svg class="logo" viewBox="0 0 512 512" aria-hidden="true"><rect width="512" height="512" rx="112" fill="#1b2024"/><path d="M274 96 C 352 170 394 240 400 322 L274 322 Z" fill="#eef3f4"/><path d="M238 150 C 198 214 176 268 168 322 L238 322 Z" fill="#56d39b"/><path d="M84 332 L428 332 C 420 402 352 430 256 430 C 160 430 92 402 84 332 Z" fill="#27b58b"/></svg>`;

/**
 * 页壳。`path` 是这一页的语言无关路径（如 `/pods/qrfix`），
 * 用来生成语言切换链接和 hreflang —— 切语言应该停在同一页，不是甩回首页。
 */
function page(lang, path, title, desc, body) {
  const t = T[lang];
  const home = lang === "en" ? "/en" : "/";
  const manifesto = lang === "en" ? "/en/manifesto" : "/manifesto";
  const zhHref = path === "/" ? "/" : path;
  const enHref = path === "/" ? "/en" : `/en${path}`;
  return `<!doctype html>
<html lang="${t.htmlLang}">
<meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<title>${esc(title)}</title>
<meta name="description" content="${esc(desc)}">
<meta property="og:title" content="${esc(title)}">
<meta property="og:description" content="${esc(desc)}">
<meta property="og:type" content="website">
<link rel="canonical" href="${SITE}${lang === "en" ? enHref : zhHref}">
<link rel="alternate" hreflang="zh-CN" href="${SITE}${zhHref}">
<link rel="alternate" hreflang="en" href="${SITE}${enHref}">
<link rel="alternate" hreflang="x-default" href="${SITE}${zhHref}">
<style>${CSS}</style>
<header class="top"><div class="wrap">
  ${LOGO}
  <div class="brand">${lang === "en" ? "PodApp" : "泊舟 AI 小程序"}<small>${lang === "en" ? "泊舟" : "PodApp"}</small></div>
  <nav>
    <a href="${home}">${t.nav.home}</a>
    <a href="${manifesto}">${t.nav.manifesto}</a>
    <a class="nav-optional" href="${LATEST}">${t.nav.download}</a>
    <a class="nav-optional" href="${REPO}">${t.nav.source}</a>
    <a class="lang" href="${t.otherHref(path)}" hreflang="${lang === "en" ? "zh-CN" : "en"}">${t.other}</a>
  </nav>
</div></header>
${body}
<footer><div class="wrap">
  ${lang === "en" ? "PodApp" : "泊舟 PodApp"} · Apache-2.0 ·
  <a href="${manifesto}">${t.nav.manifesto}</a> ·
  <a href="${REPO}">GitHub</a> · <a href="${PROTO}">${t.specLink}</a>
  <br>${t.footer}
</div></footer>
</html>`;
}

/** 读一个 Pod 的清单 + 动作契约。 */
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
  return { dir, ...m.pod, ui: m.ui ?? {}, permissions: m.permissions ?? {}, i18n: m.i18n ?? {}, actions };
}

/**
 * 取某语言下的字段。英文缺失就**回落到中文**而不是留空 ——
 * 一个空的英文页比一个混着中文的英文页更没用。
 */
function field(p, lang, key) {
  if (lang === "en") return p.i18n?.en?.[key] ?? p[key];
  return p[key];
}

function humanPerms(perms, t) {
  const out = [];
  const ai = perms.ai ?? {};
  if (ai.image_generate || ai.image_edit || ai.chat || ai.video_generate) out.push(t.permAi);
  if (perms.fs?.save_dialog) out.push(t.permSave);
  if (perms.fs?.open_dialog) out.push(t.permOpen);
  if ((perms.net?.allow ?? []).length) out.push(t.permNet(perms.net.allow.join(" ")));
  for (const h of perms.host_actions ?? []) out.push(t.permHost(h));
  return out.length ? out : [t.noPerm];
}

const pods = readdirSync(join(root, "pods"), { withFileTypes: true })
  .filter((d) => d.isDirectory())
  .map((d) => readPod(d.name))
  .sort((a, b) => a.id.localeCompare(b.id));

function indexPage(lang) {
  const t = T[lang];
  const prefix = lang === "en" ? "/en" : "";
  const manifesto = `${prefix}/manifesto`;
  const cards = pods
    .map(
      (p) => `  <a class="card" href="${prefix}/pods/${esc(p.slug)}">
    <b>${esc(field(p, lang, "name"))}</b>
    <span>${esc(field(p, lang, "summary"))}</span>
    <em>v${esc(p.version)}</em>
  </a>`,
    )
    .join("\n");

  return page(
    lang,
    "/",
    lang === "en"
      ? "PodApp — AI generates. PodApp finishes."
      : "泊舟 AI 小程序 PodApp —— AI 负责生成，PodApp 负责完成",
    t.lead,
    `<div class="hero"><div class="wrap">
  <h1>${t.heroTitle}</h1>
  <div class="tag">${esc(t.tagline)}</div>
  <p class="lead">${esc(t.lead)}</p>
  <div class="cta">
    <a class="btn primary" href="${LATEST}">${esc(t.ctaDownload)}</a>
    <a class="btn ghost" href="${manifesto}">${esc(t.ctaManifesto)}</a>
  </div>
  <p class="note">${esc(t.note)}</p>
  <div class="flow">${t.flow.map((s) => `<i>${esc(s)}</i>`).join("")}</div>
</div></div>

<section><div class="wrap">
  <h2>${esc(t.podsTitle(pods.length))}</h2>
  <p>${esc(t.podsLead)}</p>
  <div class="grid">
${cards}
  </div>
</div></section>

<section><div class="wrap">
  <h2>${esc(t.howTitle)}</h2>
  <p>${t.howLead}</p>
  <ul>${t.howList.map((x) => `<li>${x}</li>`).join("")}</ul>
</div></section>

<section><div class="wrap">
  <h2>${esc(t.aiTitle)}</h2>
  <p>${t.aiLead}</p>
  <pre><code>podapp action run app.nine-grid.image.split --json \\
  --input '{"image":"poster.png","rows":3,"cols":3,"zip":true}'</code></pre>
</div></section>

<section><div class="wrap">
  <h2>${esc(t.makeTitle)}</h2>
  <p>${t.makeLead}</p>
  <pre><code>npx podapp create my-pod
npx podapp pack my-pod</code></pre>
</div></section>`,
  );
}

function manifestoPage(lang) {
  const filename = lang === "en" ? "MANIFESTO.md" : "MANIFESTO.zh-CN.md";
  const source = readFileSync(join(root, filename), "utf8");
  const title = lang === "en" ? "The AI GUI Manifesto — PodApp" : "AI GUI 宣言 — PodApp";
  const desc =
    lang === "en"
      ? "A deterministic interaction layer between human intent and probabilistic intelligence."
      : "在人的意图与概率智能之间，建立确定性交互层。";
  return page(
    lang,
    "/manifesto",
    title,
    desc,
    `<article class="manifesto wrap">${renderMarkdown(source)}</article>`,
  );
}

function podPage(lang, p) {
  const t = T[lang];
  const home = lang === "en" ? "/en" : "/";
  const acts = p.actions.length
    ? `<h2>${esc(t.actionsTitle)}</h2>
  <p>${esc(t.actionsLead)}</p>
  <ul>${p.actions
    .map(
      (a) =>
        `<li><code>${esc(a.id)}</code>${lang === "en" && a.desc ? ` — ${esc(a.desc)}` : a.title ? ` —— ${esc(a.title)}` : ""}</li>`,
    )
    .join("")}</ul>`
    : "";
  const name = field(p, lang, "name");
  const summary = field(p, lang, "summary");
  return page(
    lang,
    `/pods/${p.slug}`,
    `${name} — ${lang === "en" ? "PodApp" : "泊舟 AI 小程序"}`,
    summary ?? "",
    `<section style="border-top:none;padding-top:44px"><div class="wrap">
  <a class="back" href="${home}">${esc(t.backAll)}</a>
  <h1 style="font-size:30px;letter-spacing:-.015em">${esc(name)}</h1>
  <p style="margin-top:11px;color:${C.dim}">${esc(summary)}</p>
  <div class="meta">
    <span>v${esc(p.version)}</span>
    <span>${esc(p.license ?? "MIT")}</span>
    <span>${esc(p.author ?? "PodApp")}</span>
    <span>ID ${esc(p.id)}</span>
  </div>
</div></section>

<section><div class="wrap">
  <h2>${esc(t.doesTitle)}</h2>
  <p>${esc(field(p, lang, "description") ?? summary)}</p>
</div></section>

${acts ? `<section><div class="wrap">${acts}</div></section>` : ""}

<section><div class="wrap">
  <h2>${esc(t.permsTitle)}</h2>
  <p>${t.permsLead}</p>
  <ul>${humanPerms(p.permissions, t).map((x) => `<li>${esc(x)}</li>`).join("")}</ul>
</div></section>

<section><div class="wrap">
  <h2>${esc(t.getTitle)}</h2>
  <p>${esc(t.getLead)}</p>
  <div class="cta" style="justify-content:flex-start"><a class="btn primary" href="${LATEST}">${esc(t.getBtn)}</a></div>
</div></section>`,
  );
}

// ---- 写盘 ----
rmSync(out, { recursive: true, force: true });
mkdirSync(join(out, "pods"), { recursive: true });
mkdirSync(join(out, "en", "pods"), { recursive: true });

writeFileSync(join(out, "index.html"), indexPage("zh"));
writeFileSync(join(out, "en", "index.html"), indexPage("en"));
writeFileSync(join(out, "manifesto.html"), manifestoPage("zh"));
writeFileSync(join(out, "en", "manifesto.html"), manifestoPage("en"));
for (const p of pods) {
  writeFileSync(join(out, "pods", `${p.slug}.html`), podPage("zh", p));
  writeFileSync(join(out, "en", "pods", `${p.slug}.html`), podPage("en", p));
}

// ---- JSON Schema ----
//
// 真相源在**另一个仓库**（podapp-protocol）。本地是兄弟目录，Vercel 上没有 ——
// 只读兄弟目录会静默少一个文件，清单里的 `$schema` 从此 404，而构建照样绿。
// 拉不到就**让构建失败**：发一个 schema 是死链的站比不发更坏。
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
  console.log("  schema/ ← 回源 GitHub");
}

// 按 `$id` 声明的路径再放一份：文件名是 podapp.schema.json，$id 写的是
// podapp-0.1.json，编辑器照 $id 取，只按原名放就是 404。路径从 $id 解析。
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
    console.log(`  schema/${want} ← 按 $id 另存`);
  }
}

// ---- 自动更新清单 ----
//
// 从最新 Release 镜像一份到 /latest.json，让客户端第一顺位端点是 podapp.net 自己。
// **拿不到只警告** —— 和 schema 相反，因为失败语义不同：客户端本来就会回退到 GitHub，
// 少这一份只是慢一点，不是坏掉。为一个能自愈的缺失卡住官网发布不划算。
try {
  const r = await fetch(`${REPO}/releases/latest/download/latest.json`, { redirect: "follow" });
  if (r.ok) {
    const text = await r.text();
    JSON.parse(text); // 确认是合法 JSON，别把 404 页面当清单发出去
    writeFileSync(join(out, "latest.json"), text);
    console.log("  latest.json ← 镜像自最新 Release");
  } else {
    console.warn(`  ⚠ 没镜像到 latest.json（HTTP ${r.status}）—— 客户端会回退到 GitHub`);
  }
} catch (e) {
  console.warn(`  ⚠ 没镜像到 latest.json（${e.message}）—— 客户端会回退到 GitHub`);
}

// robots + sitemap：双语站不给 sitemap，搜索引擎多半只收其中一种语言
const urls = [
  "/",
  "/en",
  "/manifesto",
  "/en/manifesto",
  ...pods.flatMap((p) => [`/pods/${p.slug}`, `/en/pods/${p.slug}`]),
];
writeFileSync(
  join(out, "sitemap.xml"),
  `<?xml version="1.0" encoding="UTF-8"?>\n<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">\n${urls
    .map((u) => `  <url><loc>${SITE}${u}</loc></url>`)
    .join("\n")}\n</urlset>\n`,
);
writeFileSync(join(out, "robots.txt"), `User-agent: *\nAllow: /\nSitemap: ${SITE}/sitemap.xml\n`);

console.log(`站点已生成到 website/`);
console.log(`  中文：1 首页 + 1 宣言页 + ${pods.length} 程序舱页`);
console.log(`  英文：1 首页 + 1 宣言页 + ${pods.length} 程序舱页（/en/）`);
console.log(`  sitemap ${urls.length} 条`);
