import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

type PodInfo = {
  id: string;
  name: string;
  version: string;
  summary: string | null;
  icon: string;
  accent: string | null;
  enabled: boolean;
  permissions: string[];
};

type DockStatus = {
  pods: PodInfo[];
  attached: boolean;
  host_title: string | null;
  expanded: boolean;
  capabilities: string[];
};

const $ = <T extends HTMLElement>(id: string) => document.getElementById(id) as T;

const boat = $<HTMLDivElement>("boat");
const panel = $<HTMLElement>("panel");
const attach = $<HTMLSpanElement>("attach");
const podList = $<HTMLUListElement>("pods");
const drop = $<HTMLDivElement>("drop");
const caps = $<HTMLSpanElement>("caps");

function render(s: DockStatus) {
  boat.hidden = s.expanded;
  panel.hidden = !s.expanded;

  attach.textContent = s.attached ? `已吸附 · ${s.host_title ?? ""}` : "独立模式";
  attach.classList.toggle("on", s.attached);
  caps.textContent = `能力：${s.capabilities.join(" · ")}`;

  podList.replaceChildren(
    ...s.pods.map((p) => {
      const li = document.createElement("li");
      li.className = "pod";

      const name = document.createElement("div");
      name.className = "name";
      name.textContent = p.name;

      const sub = document.createElement("div");
      sub.className = "sub";
      // 没有 summary 时退回权限摘要 —— 空行比什么都不显示更让人以为坏了
      sub.textContent = p.summary ?? p.permissions.join("、") ?? "";

      li.append(name, sub);
      li.onclick = () => invoke("dock_open_pod", { id: p.id }).catch(warn);
      return li;
    }),
  );

  if (s.pods.length === 0) {
    const li = document.createElement("li");
    li.className = "empty";
    li.textContent = "还没有 AI 小程序。把 .pod 小程序包拖进来即可安装。";
    podList.append(li);
  }
}

function warn(e: unknown) {
  // 失败要看得见。静默 catch 会让「点了没反应」变成查半天的谜。
  drop.textContent = String(e);
  drop.classList.add("bad");
  setTimeout(() => {
    drop.classList.remove("bad");
    drop.textContent = "把 AI 生成的图 / 网页 / .pod 小程序包拖到这里";
  }, 4000);
}

async function refresh() {
  try {
    render(await invoke<DockStatus>("dock_status"));
  } catch (e) {
    warn(e);
  }
}

boat.onclick = () => invoke("dock_expand", { on: true }).then(refresh).catch(warn);
$("collapse").onclick = () => invoke("dock_expand", { on: false }).then(refresh).catch(warn);

// 拖进来的文件由后端判断怎么处理。前端不猜类型 —— 猜错了就是两份判断逻辑。
listen<string[]>("dock://dropped", async (e) => {
  for (const path of e.payload) {
    if (!path.toLowerCase().endsWith(".pod")) continue;
    try {
      const p = await invoke<PodInfo>("dock_install", { path });
      drop.textContent = `装好了：${p.name} v${p.version}`;
    } catch (err) {
      warn(err);
    }
  }
  refresh();
});

// 位置变了顺带刷一次吸附状态，省得用户盯着「未吸附」以为坏了
listen("dock://placed", refresh);

refresh();
