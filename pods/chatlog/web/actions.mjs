// 对话导出。不碰 DOM：界面和 AI 无头调用 import 的是同一个它。
//
// 这个程序舱**读不到** ~/.codex —— 动作模块跑在只准读自己目录的沙箱里，那是故意的。
// 所以拿数据走宿主动作（清单里申报过，装包时会明明白白列给用户看）。
//
// 但格式转换留在这里，不放宿主：改导出样式是最常见的需求，
// 放在 Pod 里意味着改一个文件重打包就行，不用动宿主也不用发新版。

import { ACTION, defineActions } from "./action-parity.generated.mjs";

const esc = (s) =>
  String(s).replace(/[&<>]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;" })[c]);

/** rollout 的一行行 JSON → 干净的消息列表。**丢掉 developer/system**（系统提示词）。 */
function parseJsonl(text) {
  const msgs = [];
  let meta = {};
  for (const line of String(text).split("\n")) {
    const t = line.trim();
    if (!t) continue;
    let d;
    // 坏行跳过：文件可能正被 Codex 追写，最后一行是半条
    try { d = JSON.parse(t); } catch { continue; }
    if (d.type === "session_meta" && d.payload) meta = d.payload;
    const p = d.payload;
    if (!p || !p.role || p.role === "developer" || p.role === "system") continue;
    const c = p.content;
    const text2 = typeof c === "string"
      ? c
      : Array.isArray(c)
        ? c.map((x) => (typeof x === "string" ? x : (x?.text ?? ""))).join("\n")
        : "";
    if (text2.trim()) msgs.push({ role: p.role, text: text2.trim(), at: d.timestamp ?? "" });
  }
  return { meta, msgs };
}

function toMarkdown(title, meta, msgs) {
  const head = [
    `# ${title}`,
    "",
    meta.cwd ? `- 工作目录：\`${meta.cwd}\`` : "",
    meta.timestamp || meta.started ? `- 开始于：${meta.timestamp ?? meta.started}` : "",
    meta.cli_version ? `- Codex：${meta.cli_version}` : "",
    `- ${msgs.length} 条消息`,
    "",
    "---",
    "",
  ].filter(Boolean);

  const body = msgs.map((m) => {
    const who = m.role === "user" ? "🧑 用户" : "🤖 Codex";
    // 用引用块包住正文：对话里常有 ``` 代码块，直接摊平会和外层标题打架
    return `### ${who}\n\n${m.text}\n`;
  });
  return head.concat(body).join("\n");
}

function toHtml(title, meta, msgs) {
  const rows = msgs
    .map(
      (m) => `<article class="${m.role}">
  <h3>${m.role === "user" ? "🧑 用户" : "🤖 Codex"}</h3>
  <pre>${esc(m.text)}</pre>
</article>`,
    )
    .join("\n");
  // 单文件、无外链：导出的东西要能直接发给别人，依赖 CDN 的页面在断网时是白的
  return `<!doctype html>
<html lang="zh-CN"><head><meta charset="utf-8"><title>${esc(title)}</title>
<style>
body{max-width:820px;margin:40px auto;padding:0 16px;background:#14161a;color:#e6e8ec;
     font:15px/1.7 "Microsoft YaHei UI",system-ui,sans-serif}
h1{font-size:22px} h3{font-size:13px;color:#8b93a1;margin:18px 0 6px}
article{border-left:3px solid #262a31;padding-left:14px;margin:14px 0}
article.user{border-color:#38bdf8}
pre{white-space:pre-wrap;word-break:break-word;margin:0;font:inherit}
.meta{color:#8b93a1;font-size:12px}
</style></head><body>
<h1>${esc(title)}</h1>
<p class="meta">${esc(meta.cwd ?? "")} · ${esc(meta.timestamp ?? meta.started ?? "")} · ${msgs.length} 条消息</p>
${rows}
</body></html>`;
}

export default defineActions({
  [ACTION.APP_CHATLOG_SESSION_LIST]: async (input, ctx) => {
    const r = await ctx.pod.action("host.codex.session.list", { limit: input.limit ?? 50 });
    return { ok: true, count: r.count, sessions: r.sessions, message: `找到 ${r.count} 个会话` };
  },

  [ACTION.APP_CHATLOG_SESSION_EXPORT]: async (input, ctx) => {
    const pod = ctx.pod;
    let meta, msgs;

    if (input.jsonl) {
      // 兜底路径：用户自己把文件内容递进来。**不依赖 Codex 装在哪、版本是几** ——
      // 上游改了目录结构时，这条还能用。
      ({ meta, msgs } = parseJsonl(input.jsonl));
    } else if (input.session) {
      const r = await pod.action("host.codex.session.read", { session: input.session });
      meta = { cwd: r.cwd, timestamp: r.started, cli_version: r.cli_version };
      msgs = r.messages;
    } else {
      throw new Error("要导哪一个？给 session（会话 id）或 jsonl（直接给文件内容）");
    }

    if (!msgs.length) {
      throw new Error("这次会话里没有可导出的对话（系统提示词不算）");
    }

    const fmt = input.format ?? "markdown";
    const title = input.title?.trim() || msgs.find((m) => m.role === "user")?.text.split("\n")[0].slice(0, 60) || "Codex 会话";
    const text = fmt === "html" ? toHtml(title, meta, msgs) : toMarkdown(title, meta, msgs);

    const art = await pod.artifact.emit({
      kind: "text",
      action: ACTION.APP_CHATLOG_SESSION_EXPORT,
      // artifact 收的是 data URL；文本也走同一条路，宿主那边只认这一种入口
      data: "data:text/plain;base64," + btoa(unescape(encodeURIComponent(text))),
      message: `${title} · ${msgs.length} 条 · ${fmt}`,
    });

    return {
      ok: true,
      title,
      format: fmt,
      count: msgs.length,
      bytes: text.length,
      artifact: art,
      message: `已导出「${title}」${msgs.length} 条消息`,
    };
  },
});

export { parseJsonl, toMarkdown, toHtml };
