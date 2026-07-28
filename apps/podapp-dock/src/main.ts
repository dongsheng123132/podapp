import { invoke } from "@tauri-apps/api/core";
import { PhysicalPosition } from "@tauri-apps/api/dpi";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import {
  applySkin,
  builtinSkins,
  loadCustomSkins,
  parseSkin,
  saveCustomSkins,
  type DockSkin,
} from "./skins";

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
  host_available: boolean;
  host_title: string | null;
  expanded: boolean;
  placement: "attached" | "free";
  snap_edge: string | null;
  capabilities: string[];
};

type DockPlacement = {
  placement: "attached" | "free";
  snap_edge: string | null;
  attached: boolean;
  host_available: boolean;
  host_title: string | null;
  x: number;
  y: number;
  width: number;
  height: number;
};

type View = "home" | "developer" | "skins";

const POSITION_KEY = "podapp.dock-position.v1";
const ACTIVE_SKIN_KEY = "podapp.active-skin.v1";
const EDGE_LABELS: Record<string, string> = {
  left: "左侧",
  right: "右侧",
  top: "顶部",
  bottom: "底部",
  "top-left": "左上角",
  "top-right": "右上角",
  "bottom-left": "左下角",
  "bottom-right": "右下角",
};

const $ = <T extends HTMLElement>(id: string) => document.getElementById(id) as T;
const boatShell = $<HTMLElement>("boatShell");
const boat = $<HTMLButtonElement>("boat");
const panel = $<HTMLElement>("panel");
const dragBar = $<HTMLElement>("dragBar");
const attach = $<HTMLSpanElement>("attach");
const reattach = $<HTMLButtonElement>("reattach");
const podList = $<HTMLUListElement>("pods");
const drop = $<HTMLDivElement>("drop");
const caps = $<HTMLSpanElement>("caps");
const home = $<HTMLElement>("home");
const developerPanel = $<HTMLElement>("developerPanel");
const skinPanel = $<HTMLElement>("skinPanel");
const copyPrompt = $<HTMLButtonElement>("copyDeveloperPrompt");
const copyStatus = $<HTMLParagraphElement>("copyStatus");
const skinList = $<HTMLDivElement>("skinList");
const skinFile = $<HTMLInputElement>("skinFile");
const skinStatus = $<HTMLParagraphElement>("skinStatus");
const boatMark = $<HTMLSpanElement>("boatMark");
const brandMark = $<HTMLSpanElement>("brandMark");
const appWindow = getCurrentWindow();

let activeView: View = "home";
let developerPrompt = "";
let skinPrompt = "";
let customSkins = loadCustomSkins();
let activeSkinId = localStorage.getItem(ACTIVE_SKIN_KEY) ?? builtinSkins[0].id;
let cachedWindow = { x: 0, y: 0, scale: 1, ready: false };

type DragSession = {
  pointerId: number;
  surface: HTMLElement;
  startScreenX: number;
  startScreenY: number;
  originX: number | null;
  originY: number | null;
  scale: number;
  lastMove: Promise<void>;
  moved: boolean;
};

let dragSession: DragSession | null = null;

function allSkins() {
  const byId = new Map<string, DockSkin>();
  for (const skin of [...builtinSkins, ...customSkins]) byId.set(skin.id, skin);
  return [...byId.values()];
}

function selectSkin(skin: DockSkin) {
  activeSkinId = skin.id;
  localStorage.setItem(ACTIVE_SKIN_KEY, skin.id);
  applySkin(skin);
  boatMark.textContent = skin.mark;
  brandMark.textContent = skin.mark;
  renderSkins();
}

function renderSkins() {
  skinList.replaceChildren(...allSkins().map((skin) => {
    const button = document.createElement("button");
    button.type = "button";
    button.className = "skin-row";
    button.classList.toggle("selected", skin.id === activeSkinId);

    const swatch = document.createElement("span");
    swatch.className = "skin-swatch";
    swatch.textContent = skin.mark;
    swatch.style.background = skin.colors.markBackground;
    swatch.style.color = skin.colors.foreground;

    const label = document.createElement("span");
    label.className = "skin-label";
    const name = document.createElement("b");
    name.textContent = skin.name;
    const author = document.createElement("small");
    author.textContent = `${skin.author} · v${skin.version}`;
    label.append(name, author);

    const selected = document.createElement("span");
    selected.className = "skin-check";
    selected.textContent = skin.id === activeSkinId ? "✓" : "";
    button.append(swatch, label, selected);
    button.onclick = () => selectSkin(skin);
    return button;
  }));
}

function applySavedSkin() {
  const skins = allSkins();
  selectSkin(skins.find((skin) => skin.id === activeSkinId) ?? builtinSkins[0]);
}

async function writeClipboard(text: string) {
  try {
    await navigator.clipboard.writeText(text);
  } catch {
    const textarea = document.createElement("textarea");
    textarea.value = text;
    textarea.style.position = "fixed";
    textarea.style.opacity = "0";
    document.body.append(textarea);
    textarea.select();
    const copied = document.execCommand("copy");
    textarea.remove();
    if (!copied) throw new Error("复制失败，请检查系统剪贴板权限");
  }
}

function setView(view: View, expanded: boolean) {
  activeView = view;
  home.hidden = !expanded || view !== "home";
  developerPanel.hidden = !expanded || view !== "developer";
  skinPanel.hidden = !expanded || view !== "skins";
}

function setEdgeClass(element: HTMLElement, placement: string, edge: string | null) {
  for (const name of [...Object.keys(EDGE_LABELS).map((item) => `edge-${item}`), "free"]) {
    element.classList.remove(name);
  }
  if (placement === "free") element.classList.add("free");
  if (edge) element.classList.add(`edge-${edge}`);
  else if (placement === "attached") element.classList.add("edge-right");
}

function render(s: DockStatus) {
  boatShell.hidden = s.expanded;
  panel.hidden = !s.expanded;
  setView(activeView, s.expanded);
  setEdgeClass(boatShell, s.placement, s.snap_edge);
  setEdgeClass(panel, s.placement, s.snap_edge);

  if (s.placement === "free") {
    attach.textContent = `自由漂浮${s.snap_edge ? ` · ${EDGE_LABELS[s.snap_edge] ?? s.snap_edge}` : ""}`;
  } else if (s.attached) {
    attach.textContent = `已吸附 · ${s.host_title || "宿主"}`;
  } else {
    attach.textContent = "等待宿主 · 屏幕右侧";
  }
  attach.classList.toggle("on", s.attached);
  reattach.hidden = s.placement !== "free";
  reattach.disabled = false;
  reattach.title = s.host_available ? "重新吸附宿主" : "切回自动吸附等待态";
  caps.textContent = `${s.pods.length} 个 Pod · ${s.capabilities.length} 项能力`;

  podList.replaceChildren(...s.pods.map((pod) => {
    const li = document.createElement("li");
    li.className = "pod";
    const name = document.createElement("div");
    name.className = "name";
    name.textContent = pod.name;
    const sub = document.createElement("div");
    sub.className = "sub";
    sub.textContent = pod.summary ?? pod.permissions.join("、");
    li.append(name, sub);
    li.onclick = () => invoke("dock_open_pod", { id: pod.id }).catch(warn);
    return li;
  }));

  if (s.pods.length === 0) {
    const li = document.createElement("li");
    li.className = "empty";
    li.textContent = "还没有 Pod";
    podList.append(li);
  }
}

function warn(error: unknown) {
  drop.textContent = String(error);
  drop.classList.add("bad");
  setTimeout(() => {
    drop.classList.remove("bad");
    drop.textContent = "拖入图像、网页或 .pod";
  }, 4000);
}

async function refresh() {
  try {
    render(await invoke<DockStatus>("dock_status"));
    await syncWindowMetrics();
  } catch (error) {
    warn(error);
  }
}

async function syncWindowMetrics() {
  const [position, scale] = await Promise.all([
    appWindow.outerPosition(),
    appWindow.scaleFactor(),
  ]);
  cachedWindow = { x: position.x, y: position.y, scale, ready: true };
}

function savePosition(position: DockPlacement) {
  localStorage.setItem(POSITION_KEY, JSON.stringify({
    x: position.x,
    y: position.y,
    edge: position.snap_edge,
  }));
}

function beginPointerDrag(event: PointerEvent) {
  if (event.button !== 0 || (event.target as HTMLElement).closest("button")) return;
  event.preventDefault();
  const surface = event.currentTarget as HTMLElement;
  surface.setPointerCapture(event.pointerId);
  const session: DragSession = {
    pointerId: event.pointerId,
    surface,
    startScreenX: event.screenX,
    startScreenY: event.screenY,
    originX: cachedWindow.ready ? cachedWindow.x : null,
    originY: cachedWindow.ready ? cachedWindow.y : null,
    scale: cachedWindow.scale,
    lastMove: Promise.resolve(),
    moved: false,
  };
  dragSession = session;
  invoke("dock_begin_drag").catch(warn);
  if (!cachedWindow.ready) {
    syncWindowMetrics().then(() => {
      if (dragSession !== session) return;
      session.originX = cachedWindow.x;
      session.originY = cachedWindow.y;
      session.scale = cachedWindow.scale;
    }).catch(warn);
  }
}

function continuePointerDrag(event: PointerEvent) {
  const session = dragSession;
  if (
    !session
    || event.pointerId !== session.pointerId
    || session.originX === null
    || session.originY === null
  ) return;
  const dx = event.screenX - session.startScreenX;
  const dy = event.screenY - session.startScreenY;
  if (Math.abs(dx) < 2 && Math.abs(dy) < 2) return;
  session.moved = true;
  const x = Math.round(session.originX + dx * session.scale);
  const y = Math.round(session.originY + dy * session.scale);
  cachedWindow = { x, y, scale: session.scale, ready: true };
  session.lastMove = appWindow
    .setPosition(new PhysicalPosition(x, y))
    .catch(warn);
}

async function endPointerDrag(event: PointerEvent) {
  const session = dragSession;
  if (!session || event.pointerId !== session.pointerId) return;
  dragSession = null;
  if (session.surface.hasPointerCapture(event.pointerId)) {
    session.surface.releasePointerCapture(event.pointerId);
  }
  if (!session.moved) {
    await invoke("dock_cancel_drag").catch(warn);
    return;
  }
  try {
    await session.lastMove;
    // WebView 的 Promise 表示移动请求已送达，不保证 Windows 已更新 GetWindowRect。
    // 等两个合成帧再读，避免把松手前的旧坐标拿去做磁吸判断。
    await new Promise((resolve) => window.setTimeout(resolve, 34));
    const position = await appWindow.outerPosition();
    const result = await invoke<DockPlacement>("dock_finish_drag", {
      x: position.x,
      y: position.y,
    });
    savePosition(result);
    cachedWindow = {
      x: result.x,
      y: result.y,
      scale: session.scale,
      ready: true,
    };
    await refresh();
  } catch (error) {
    warn(error);
  }
}

for (const surface of [boatShell, dragBar]) {
  surface.addEventListener("pointerdown", beginPointerDrag);
  surface.addEventListener("pointermove", continuePointerDrag);
  surface.addEventListener("pointerup", endPointerDrag);
  surface.addEventListener("pointercancel", endPointerDrag);
}

async function restorePosition() {
  try {
    const saved = JSON.parse(localStorage.getItem(POSITION_KEY) ?? "null") as {
      x?: unknown;
      y?: unknown;
      edge?: unknown;
    } | null;
    if (!saved || typeof saved.x !== "number" || typeof saved.y !== "number") return;
    await invoke("dock_restore_free", {
      x: Math.round(saved.x),
      y: Math.round(saved.y),
      edge: typeof saved.edge === "string" ? saved.edge : null,
    });
  } catch {
    localStorage.removeItem(POSITION_KEY);
  }
}

boat.onclick = () => invoke("dock_expand", { on: true }).then(refresh).catch(warn);
$("collapse").onclick = () => {
  activeView = "home";
  invoke("dock_expand", { on: false }).then(refresh).catch(warn);
};
reattach.onclick = async () => {
  try {
    await invoke("dock_attach");
    localStorage.removeItem(POSITION_KEY);
    await refresh();
  } catch (error) {
    warn(error);
  }
};

$("developer").onclick = async () => {
  setView("developer", true);
  try {
    developerPrompt ||= await invoke<string>("dock_developer_prompt");
  } catch (error) {
    warn(error);
  }
};
$("developerBack").onclick = () => setView("home", true);
$("skins").onclick = () => {
  setView("skins", true);
  renderSkins();
};
$("skinBack").onclick = () => setView("home", true);

copyPrompt.onclick = async () => {
  try {
    developerPrompt ||= await invoke<string>("dock_developer_prompt");
    await writeClipboard(developerPrompt);
    copyPrompt.textContent = "已复制";
    copyStatus.textContent = "修改第一行需求后交给 AI。";
    setTimeout(() => { copyPrompt.textContent = "复制完整开发指令"; }, 2200);
  } catch (error) {
    warn(error);
  }
};

$("importSkin").onclick = () => skinFile.click();
skinFile.onchange = async () => {
  const file = skinFile.files?.[0];
  if (!file) return;
  try {
    const skin = parseSkin(JSON.parse(await file.text()));
    customSkins = [...customSkins.filter((item) => item.id !== skin.id), skin];
    saveCustomSkins(customSkins);
    selectSkin(skin);
    skinStatus.textContent = `已导入 ${skin.name}`;
  } catch (error) {
    skinStatus.textContent = String(error);
  } finally {
    skinFile.value = "";
  }
};
$("copySkinPrompt").onclick = async () => {
  try {
    skinPrompt ||= await invoke<string>("dock_skin_prompt");
    await writeClipboard(skinPrompt);
    skinStatus.textContent = "皮肤规范已复制";
  } catch (error) {
    skinStatus.textContent = String(error);
  }
};

listen<string[]>("dock://dropped", async (event) => {
  for (const path of event.payload) {
    if (!path.toLowerCase().endsWith(".pod")) continue;
    try {
      const pod = await invoke<PodInfo>("dock_install", { path });
      drop.textContent = `已安装 ${pod.name} v${pod.version}`;
    } catch (error) {
      warn(error);
    }
  }
  refresh();
});

listen<DockPlacement>("dock://placed", (event) => {
  if (event.payload.placement === "free") savePosition(event.payload);
  refresh();
});

applySavedSkin();
restorePosition().then(refresh);
