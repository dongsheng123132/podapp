//! 宿主桥 —— 注入进程序舱页面和无头 runner 的那层 API。
//!
//! **程序舱自己不写 `<script src=…>`。** 桥由宿主注入，于是 AI 生成的程序舱不可能漏写，
//! 内嵌 iframe 与独立窗口两种容器的行为也不会分叉。
//!
//! ## 两个名字，一个对象
//!
//! 桥同时挂 `window.pod`（PodApp Protocol 的规范名）和 `window.<宿主品牌>`（别名）。
//! 理由是「One Pod, Any AI」不能只是口号：一个照着 `pod.*` 写的程序舱必须能在 U-King 里跑，
//! 而 U-King 0.9.72 已经发出去的三个小程序写的是 `uking.*`，也必须继续跑。
//! 两个名字指向**同一个对象**，不是两份实现 —— 那样就又分叉了。
//!
//! ## 桥不做什么
//!
//! - **绝不下发凭据。** 程序舱永远拿不到 API Key、base URL 或鉴权头。所有 AI 调用在宿主
//!   进程里完成，程序舱只收到结果字节。
//! - **绝不代理任意网络请求。** 桥上没有通用 `fetch`。

/// 给入口 HTML 注入桥脚本。
///
/// 只注一次：重复注入会让 `window.pod` 被后一个覆盖，而两次注册的事件监听会让
/// 每个回调跑两遍 —— 这种 bug 在界面上只表现为「点一次执行了两次」，极难查。
pub fn inject(html: &[u8], pod_id: &str) -> Vec<u8> {
    let p = crate::profile();
    let tag = format!(
        "<script src=\"/__{}/bridge.js\" data-pod=\"{}\"></script>",
        p.env_prefix.to_ascii_lowercase(),
        attr_escape(pod_id)
    );
    let s = String::from_utf8_lossy(html).to_string();
    let lower = s.to_ascii_lowercase();
    let out = if let Some(i) = lower.find("<head>") {
        let at = i + "<head>".len();
        format!("{}{}{}", &s[..at], tag, &s[at..])
    } else if let Some(i) = lower.find("<html") {
        let at = s[i..].find('>').map(|k| i + k + 1).unwrap_or(0);
        format!("{}{}{}", &s[..at], tag, &s[at..])
    } else {
        format!("{tag}{s}")
    };
    out.into_bytes()
}

/// HTML 属性值转义。
///
/// 上游 [`crate::manifest::valid_pod_id`] 已经把 pod id 限死在字母数字和 `._-` 里，
/// 所以正常路径上这个函数什么都不会改。但 [`inject`] 是 `pub` 的，谁都能拿任意字符串调它 ——
/// 「调用方保证过了」是这类漏洞最常见的开场白。转义放在**产生标记的地方**，
/// 而不是指望每个调用点都记得先校验。
fn attr_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            _ => out.push(c),
        }
    }
    out
}

/// 桥脚本的服务路径，如 `/__podapp/bridge.js`。宿主的协议处理器认这个路径。
pub fn script_path() -> String {
    format!(
        "/__{}/bridge.js",
        crate::profile().env_prefix.to_ascii_lowercase()
    )
}

/// 生成注入页面的桥脚本。
///
/// 刻意用 ES5 写法：程序舱页面可能被很老的 WebView 加载，桥本身不该成为兼容性风险。
pub fn script() -> String {
    let alias = &crate::profile().bridge_global;
    format!(
        r#"
(function () {{
  var POD = document.currentScript && (document.currentScript.dataset.pod || document.currentScript.dataset.app);
  function rpc(verb, args) {{
    return fetch("/rpc/" + POD + "/" + verb, {{
      method: "POST",
      headers: {{ "content-type": "application/json" }},
      body: JSON.stringify(args === undefined ? {{}} : args)
    }}).then(function (r) {{ return r.json(); }}).then(function (m) {{
      if (m && m.ok) return m.data;
      throw new Error((m && m.error) || "调用失败");
    }});
  }}
  var imageProxy = new Proxy({{}}, {{ get: function (_t, k) {{
    return function () {{ return rpc("image." + String(k), Array.prototype.slice.call(arguments)); }};
  }}}});
  function post(m) {{ try {{ parent.postMessage(m, "*"); }} catch (e) {{}} return Promise.resolve(); }}
  var api = {{
    version: "0.1",
    pod: POD,
    app: POD,
    action: function (id, input) {{ return rpc("action", {{ id: id, input: input }}); }},
    ai: {{
      imageEdit: function (p) {{ return rpc("ai.image_edit", p); }},
      imageGen: function (p) {{ return rpc("ai.image_generate", p); }},
      chat: function (p) {{ return rpc("ai.chat", p); }}
    }},
    file: {{
      save: function (name, dataUrl) {{ return rpc("file.save", {{ name: name, dataUrl: dataUrl }}); }},
      open: function (filters) {{ return rpc("file.open", {{ filters: filters }}); }}
    }},
    storage: {{
      get: function (k) {{ return rpc("storage.get", {{ key: k }}); }},
      set: function (k, v) {{ return rpc("storage.set", {{ key: k, value: v }}); }}
    }},
    artifact: {{ emit: function (p) {{ return rpc("artifact.emit", p); }} }},
    ui: {{
      toast: function (m) {{ return post({{ __pod: "toast", msg: m }}); }},
      progress: function (p, l) {{ return post({{ __pod: "progress", percent: p, label: l }}); }},
      close: function () {{ return post({{ __pod: "close" }}); }}
    }},
    image: imageProxy,
    onEvent: function (fn) {{
      window.addEventListener("message", function (e) {{
        if (e.data && (e.data.__pod_ev || e.data.__uking_ev)) fn(e.data);
      }});
    }}
  }};
  // 规范名与宿主品牌别名指向同一个对象 —— 两个名字，一份实现
  window.pod = api;
  window.{alias} = api;
}})();
"#
    )
}

/// 无头 runner（宿主生成的临时文件）。
///
/// 用 Node import 程序舱的动作模块，宿主能力经 stdout/stdin 行协议代理回 Rust ——
/// 模块自己不碰网络也不碰文件系统。**parity 的命门在这里**：GUI 那面点按钮走 rpc，
/// 而 CLI / MCP / 影核这三面没有 webview，靠的就是这段代码 import **同一个** actions 模块。
pub const RUNNER_JS: &str = r#"
// PodApp 程序舱无头 runner —— 宿主每次执行时生成，勿手改。
import { readFileSync, writeFileSync } from "node:fs";
import { createInterface } from "node:readline";

const [modPath, actionId, inFile, outFile] = process.argv.slice(2);

// 与宿主的行协议：请求走 stdout、以 \x01 打头（避开程序舱自己的 console.log）；
// 响应从 stdin 读一行。顺序严格 FIFO。
const rl = createInterface({ input: process.stdin });
const pending = [];
rl.on("line", (line) => { const r = pending.shift(); if (r) r(line); });
let seq = 0;
function call(verb, args) {
  return new Promise((resolve, reject) => {
    const id = ++seq;
    pending.push((line) => {
      let m;
      try { m = JSON.parse(line); } catch { return reject(new Error("宿主响应不是 JSON")); }
      if (m.ok) resolve(m.data); else reject(new Error(m.error || "宿主拒绝了这次调用"));
    });
    process.stdout.write("\x01" + JSON.stringify({ id, verb, args }) + "\n");
  });
}

const api = {
  version: "0.1",
  action: (id, input) => call("action", { id, input }),
  ai: {
    imageEdit: (p) => call("ai.image_edit", p),
    imageGen:  (p) => call("ai.image_generate", p),
    chat:      (p) => call("ai.chat", p),
  },
  file: {
    save: (name, dataUrl) => call("file.save", { name, dataUrl }),
    open: (filters) => call("file.open", { filters }),
  },
  storage: {
    get: (key) => call("storage.get", { key }),
    set: (key, value) => call("storage.set", { key, value }),
  },
  ui: {
    // 无头场景没有界面：进度只当日志走 stderr，绝不污染 stdout 的行协议
    toast: (msg) => { process.stderr.write("[toast] " + msg + "\n"); return Promise.resolve(); },
    progress: (p, label) => { process.stderr.write("[progress] " + p + "% " + (label || "") + "\n"); return Promise.resolve(); },
    close: () => Promise.resolve(),
  },
  image: new Proxy({}, { get: (_t, k) => (...a) => call("image." + String(k), a) }),
  artifact: { emit: (p) => call("artifact.emit", p) },
};

// 与 GUI 桥一致：规范名 + 别名指向同一个对象
const ctx = { pod: api, uking: api, signal: undefined };

try {
  const href = "file:///" + String(modPath).replace(/\\/g, "/").replace(/^\/+/, "");
  const mod = await import(href);
  const table = mod.default ?? mod;
  const fn = table[actionId];
  if (typeof fn !== "function")
    throw new Error("动作模块里没有 " + actionId + " 的实现（default 导出的表里找不到这个 key）");
  const input = JSON.parse(readFileSync(inFile, "utf8"));
  const out = await fn(input, ctx);
  writeFileSync(outFile, JSON.stringify({ ok: true, data: out }));
  process.exit(0);
} catch (e) {
  writeFileSync(outFile, JSON.stringify({ ok: false, error: String((e && e.message) || e) }));
  process.exit(1);
}
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn injects_exactly_once_into_head() {
        let html = b"<!doctype html><html><head><title>x</title></head><body>hi</body></html>";
        let out = inject(html, "org.podapp.test");
        let s = String::from_utf8(out).unwrap();
        assert_eq!(s.matches("bridge.js").count(), 1, "只能注一次");
        assert!(s.contains("data-pod=\"org.podapp.test\""));
        assert!(
            s.find("bridge.js").unwrap() < s.find("<body>").unwrap(),
            "必须在 body 之前"
        );
    }

    #[test]
    fn injects_into_html_without_head() {
        // 没有 <head> 的页面也得拿到桥，否则「点了没反应」而且查不出原因
        for html in [&b"<html><body>hi</body></html>"[..], &b"just text"[..]] {
            let s = String::from_utf8(inject(html, "p")).unwrap();
            assert_eq!(s.matches("bridge.js").count(), 1, "缺 head 也要注入：{s}");
        }
    }

    #[test]
    fn a_hostile_pod_id_cannot_break_out_of_the_attribute() {
        let s =
            String::from_utf8(inject(b"<head></head>", "evil\"><script>bad()</script>")).unwrap();

        // 真正的失败模式是「攻击者的字符原样落进标记里」。只检查 `<script>bad()`
        // 这类子串是自欺欺人 —— 它在带引号的属性值里本来就无害，
        // 而真正危险的那个 `"` 反倒检查不到。所以断言的是：标记里没有任何生字符。
        let injected_tag = &s[..s.find("></script>").unwrap()];
        // 转义保证了值里没有生引号，所以 `data-pod="` 之后的收尾引号就是分隔符本身
        let value = injected_tag
            .split("data-pod=\"")
            .nth(1)
            .and_then(|v| v.strip_suffix('"'))
            .expect("data-pod 属性该是带引号的");
        for c in ['"', '<', '>'] {
            assert!(!value.contains(c), "属性值里出现生字符 {c:?}：{value}");
        }
        assert!(
            s.contains("&lt;script&gt;"),
            "危险字符该被转义而不是丢掉：{s}"
        );
        // 页面里仍然只有我们注入的那一个 script 元素
        assert_eq!(s.matches("<script ").count(), 1);
    }

    #[test]
    fn normal_pod_ids_pass_through_untouched() {
        // 合法 id 只含字母数字和 ._- ，转义不该动它们，否则 data-pod 与 RPC 路径对不上
        let s = String::from_utf8(inject(b"<head></head>", "org.podapp.image.nine-grid")).unwrap();
        assert!(s.contains("data-pod=\"org.podapp.image.nine-grid\""), "{s}");
    }

    #[test]
    fn script_exposes_both_names_and_no_generic_fetch() {
        let js = script();
        assert!(js.contains("window.pod = api"), "规范名");
        assert!(
            js.contains(&format!("window.{} = api", crate::profile().bridge_global)),
            "品牌别名"
        );
        for verb in ["artifact", "/rpc/", "image.", "storage.get"] {
            assert!(js.contains(verb), "桥缺少 {verb}");
        }
        // 桥上不该有通用 fetch 出口：唯一的 fetch 是打到自己的 /rpc/ 上
        assert_eq!(
            js.matches("fetch(").count(),
            1,
            "桥上只该有一处 fetch，且指向 /rpc/"
        );
    }

    #[test]
    fn runner_gives_actions_module_both_names() {
        assert!(RUNNER_JS.contains("pod: api"));
        assert!(RUNNER_JS.contains("uking: api"));
        assert!(RUNNER_JS.contains("\\x01"), "行协议前缀不能丢");
    }
}
