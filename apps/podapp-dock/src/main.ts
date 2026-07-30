import { invoke } from "@tauri-apps/api/core";
import { PhysicalPosition } from "@tauri-apps/api/dpi";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { openUrl } from "@tauri-apps/plugin-opener";
import { relaunch } from "@tauri-apps/plugin-process";
import { check } from "@tauri-apps/plugin-updater";
import { Pet, spriteUrl } from "./pet";
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

/** 一条流程验完之后的样子。跟 Rust 侧 `dock_flow_check` 一一对应。 */
type FlowPlan = {
  id: string;
  name: string;
  problems: string[];
  steps: { action: string; title: string; confirm: boolean; input: unknown }[];
};

/** 跑到哪儿了。形状由 `podapp_flow::Outcome::to_json` 决定 —— 只有一份定义。 */
type FlowOutcome = {
  state: "done" | "needs_confirm" | "failed";
  step?: number;
  action?: string;
  title?: string;
  error?: string;
  resumeFrom?: number;
  results: unknown[];
};

/** MCP 桥的现状。跟 Rust 侧 `dock_mcp` 一一对应。 */
type McpInfo = {
  present: boolean;
  path: string;
  tools: number;
  claude: string;
  codexToml: string;
};

/** 一只 Codex 宠物。跟 `podapp_codex::pets::PetInfo::to_json` 一一对应。 */
type PetSummary = {
  id: string;
  displayName: string;
  description: string;
  bytes: number;
};

type View = "home" | "developer" | "skins" | "mcp" | "flow";

const POSITION_KEY = "podapp.dock-position.v1";
const ACTIVE_SKIN_KEY = "podapp.active-skin.v1";
/** 选中的宠物**独立于皮肤存**：换个配色不该把宠物弄丢。 */
const ACTIVE_PET_KEY = "podapp.active-pet.v1";
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
const petList = $<HTMLDivElement>("petList");
const mcpPanel = $<HTMLElement>("mcpPanel");
const mcpState = $<HTMLParagraphElement>("mcpState");
const mcpPath = $<HTMLElement>("mcpPath");
const mcpStatus = $<HTMLParagraphElement>("mcpStatus");
const mcpSub = $<HTMLElement>("mcpSub");
const flowPanel = $<HTMLElement>("flowPanel");
const flowJson = $<HTMLTextAreaElement>("flowJson");
const flowSteps = $<HTMLDivElement>("flowSteps");
const flowStatus = $<HTMLParagraphElement>("flowStatus");
const flowConfirm = $<HTMLDivElement>("flowConfirm");
const flowConfirmText = $<HTMLParagraphElement>("flowConfirmText");
const petHint = $<HTMLElement>("petHint");
const boatMark = $<HTMLSpanElement>("boatMark");
const brandMark = $<HTMLSpanElement>("brandMark");
const appWindow = getCurrentWindow();

let activeView: View = "home";
let developerPrompt = "";
let skinPrompt = "";
let flowPrompt = "";
let flowLastOutcome: FlowOutcome | null = null;
let customSkins = loadCustomSkins();
let activeSkinId = localStorage.getItem(ACTIVE_SKIN_KEY) ?? builtinSkins[0].id;
let cachedWindow = { x: 0, y: 0, scale: 1, ready: false };

const pet = new Pet();
let pets: PetSummary[] = [];
let mcp: McpInfo | null = null;
let activePetId = localStorage.getItem(ACTIVE_PET_KEY);

type DragSession = {
  pointerId: number;
  surface: HTMLElement;
  startScreenX: number;
  startScreenY: number;
  /** 上一帧的横坐标 —— 宠物朝哪边跑看的是它，不是起点 */
  lastScreenX: number;
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

function activeSkin(): DockSkin {
  return allSkins().find((skin) => skin.id === activeSkinId) ?? builtinSkins[0];
}

/**
 * 把宠物贴上去，或者取下来换回 emoji 标记。
 *
 * 贴上宠物时**必须清掉标记文字**：emoji 会盖在图集上面，
 * 而那看起来像"宠物身上多了个不该有的东西"，不像"两层叠了"。
 */
function applyPet() {
  const mark = activeSkin().mark;
  const chosen = activePetId && pets.some((item) => item.id === activePetId)
    ? activePetId
    : null;
  pet.mount([boatMark, brandMark], chosen);
  boatMark.textContent = chosen ? "" : mark;
  brandMark.textContent = chosen ? "" : mark;
}

function selectPet(id: string | null) {
  activePetId = id;
  if (id) localStorage.setItem(ACTIVE_PET_KEY, id);
  else localStorage.removeItem(ACTIVE_PET_KEY);
  applyPet();
  renderPets();
}

function selectSkin(skin: DockSkin) {
  activeSkinId = skin.id;
  localStorage.setItem(ACTIVE_SKIN_KEY, skin.id);
  applySkin(skin);
  // 皮肤自带宠物就顺带换过去 —— 那是皮肤作者的意图。
  // 不带的皮肤不动当前宠物：换个配色不该把宠物弄丢。
  if (skin.sprite?.pet) activePetId = skin.sprite.pet;
  applyPet();
  renderSkins();
  renderPets();
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

function renderPets() {
  const rows: HTMLButtonElement[] = [];

  const none = document.createElement("button");
  none.type = "button";
  none.className = "skin-row";
  none.classList.toggle("selected", !activePetId);
  const noneSwatch = document.createElement("span");
  noneSwatch.className = "skin-swatch";
  noneSwatch.textContent = activeSkin().mark;
  const noneLabel = document.createElement("span");
  noneLabel.className = "skin-label";
  const noneName = document.createElement("b");
  noneName.textContent = "不用宠物";
  const noneSub = document.createElement("small");
  noneSub.textContent = "只显示皮肤标记";
  noneLabel.append(noneName, noneSub);
  const noneCheck = document.createElement("span");
  noneCheck.className = "skin-check";
  noneCheck.textContent = activePetId ? "" : "✓";
  none.append(noneSwatch, noneLabel, noneCheck);
  none.onclick = () => selectPet(null);
  rows.push(none);

  for (const item of pets) {
    const button = document.createElement("button");
    button.type = "button";
    button.className = "skin-row";
    button.classList.toggle("selected", item.id === activePetId);

    // 列表里的头像就是 idle 第一帧 —— 契约里那一帧本来就是按「能当静态图用」画的，
    // 所以这里不用另出一张缩略图。
    const swatch = document.createElement("span");
    swatch.className = "skin-swatch pet-thumb";
    swatch.style.backgroundImage = `url("${spriteUrl(item.id)}")`;

    const label = document.createElement("span");
    label.className = "skin-label";
    const name = document.createElement("b");
    name.textContent = item.displayName;
    const sub = document.createElement("small");
    sub.textContent = item.description || `${Math.round(item.bytes / 1024)} KB`;
    label.append(name, sub);

    const tick = document.createElement("span");
    tick.className = "skin-check";
    tick.textContent = item.id === activePetId ? "✓" : "";
    button.append(swatch, label, tick);
    button.onclick = () => selectPet(item.id);
    rows.push(button);
  }

  petList.replaceChildren(...rows);
  petHint.textContent = pets.length
    ? `${pets.length} 只 · 拖图集可再加`
    : "拖一张 1536×1872 图集进来就有";
}

/**
 * 读一遍本机的宠物。
 *
 * **读不到就当没有，绝不打扰**：没装 Codex、或者一只宠物都没做过是常态，
 * 而不是错误。在皮肤面板上红一行「读取宠物失败」，用户什么都没做错却要被吓一次。
 */
async function loadPets() {
  try {
    pets = await invoke<PetSummary[]>("dock_pets");
  } catch {
    pets = [];
  }
  applyPet();
  renderPets();
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
  mcpPanel.hidden = !expanded || view !== "mcp";
  flowPanel.hidden = !expanded || view !== "flow";
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
    li.onclick = () => {
      // 开程序舱窗口要等 WebView2 起来，几百毫秒到一两秒不等。
      // 这段时间里界面没有任何反馈，宠物是唯一在动的东西。
      pet.set("running");
      invoke("dock_open_pod", { id: pod.id })
        .catch(warn)
        .finally(() => pet.rest());
    };
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
  // 出错时宠物也垮一下。收起态看不到那行红字（只有 64px 的浮舱），
  // 宠物是那个状态下唯一能传达「刚才没成」的东西。
  pet.once("failed");
  setTimeout(() => {
    drop.classList.remove("bad");
    drop.textContent = "拖入图像、宠物图集或 .pod";
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
    lastScreenX: event.screenX,
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
  // 拖着走时朝移动方向跑。用**本次位移相对上一帧**的方向，不是相对起点 ——
  // 相对起点的话，拖出去再拖回来，宠物会一路朝右跑着往左走。
  if (Math.abs(event.screenX - session.lastScreenX) >= 2) {
    pet.set(event.screenX > session.lastScreenX ? "running-right" : "running-left");
    session.lastScreenX = event.screenX;
  }
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
  pet.rest();
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

// 鼠标碰到收起态的浮舱就招个手。**只在指针真进来时触发一次**，
// 不用 mousemove —— 那会在整个悬停期间反复重播，看起来像抽搐而不是打招呼。
boatShell.addEventListener("pointerenter", () => pet.once("waving"));

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
  // 每次打开都重读：用户可能刚在 Codex 那边用 hatch-pet 做了一只新的。
  // 只在启动时读一次的话，得重启浮舱才看得见，而没人会想到要重启。
  loadPets();
};
$("skinBack").onclick = () => setView("home", true);
$("mcpBack").onclick = () => setView("home", true);
$("flowBack").onclick = () => setView("home", true);
$("flow").onclick = () => {
  setView("flow", true);
  flowStatus.textContent = "要新能力才做 Pod；组合已有动作用流程。";
};

let flowPlan: FlowPlan | null = null;
let flowResults: unknown[] = [];

/** 把每一步画出来。`mark` 决定第 i 步显示成什么状态。 */
function renderFlowSteps(mark: (i: number) => string) {
  if (!flowPlan) return flowSteps.replaceChildren();
  flowSteps.replaceChildren(...flowPlan.steps.map((s, i) => {
    const row = document.createElement("div");
    row.className = `flow-step ${mark(i)}`;
    const n = document.createElement("i");
    n.textContent = String(i + 1);
    const label = document.createElement("span");
    const b = document.createElement("b");
    // 没装的动作没有标题 —— 那就把动作 ID 显出来，别显示一个空行
    b.textContent = s.title || s.action;
    const sub = document.createElement("small");
    sub.textContent = s.confirm ? `${s.action} · 要确认` : s.action;
    label.append(b, sub);
    row.append(n, label);
    return row;
  }));
}

/** 读文本框里那份 JSON。**解析错要说人话** —— 小白粘错一个逗号最常见。 */
function readFlow(): unknown | null {
  try {
    return JSON.parse(flowJson.value);
  } catch (e) {
    flowStatus.textContent = `这不是一份能读的 JSON：${(e as Error).message}`;
    flowSteps.replaceChildren();
    return null;
  }
}

async function checkFlow(): Promise<boolean> {
  const flow = readFlow();
  if (!flow) return false;
  try {
    flowPlan = await invoke<FlowPlan>("dock_flow_check", { flow });
  } catch (error) {
    flowPlan = null;
    flowSteps.replaceChildren();
    flowStatus.textContent = String(error);
    return false;
  }
  renderFlowSteps(() => "");
  if (flowPlan.problems.length) {
    // 一次全列出来。原样贴回给 AI 就能一轮改完，比一条一条挤有用
    flowStatus.textContent = flowPlan.problems.join(String.fromCharCode(10));
    return false;
  }
  flowStatus.textContent = `${flowPlan.name} · ${flowPlan.steps.length} 步，可以跑`;
  return true;
}

function showOutcome(o: FlowOutcome) {
  flowLastOutcome = o;
  flowResults = o.results ?? [];
  const stopped = o.step ?? -1;
  renderFlowSteps((i) => {
    if (o.state === "needs_confirm" && i === stopped) return "confirm";
    if (o.state === "failed" && i === stopped) return "bad";
    return i < stopped || o.state === "done" ? "ok" : "";
  });

  if (o.state === "needs_confirm") {
    flowConfirm.hidden = false;
    flowConfirmText.textContent =
      `第 ${stopped + 1} 步「${o.title || o.action}」声明了要确认。跑之前先给你看一眼。`;
    flowStatus.textContent = "等你点头";
    return;
  }
  flowConfirm.hidden = true;
  flowStatus.textContent = o.state === "done"
    ? `跑完了 · ${flowResults.length} 步有结果`
    : `第 ${stopped + 1} 步失败：${o.error}`;
}

async function runFlow(from: number) {
  const flow = readFlow();
  if (!flow) return;
  flowConfirm.hidden = true;
  flowStatus.textContent = "跑着…";
  try {
    const o = await invoke<FlowOutcome>("dock_flow_run", {
      flow,
      seed: null,
      from,
      results: from === 0 ? [] : flowResults,
    });
    showOutcome(o);
  } catch (error) {
    flowStatus.textContent = String(error);
  }
}

$("flowCheck").onclick = () => { checkFlow(); };
$("flowRun").onclick = async () => {
  flowResults = [];
  if (await checkFlow()) runFlow(0);
};
$("flowYes").onclick = () => {
  // 点头之后从**宿主算好的**那一步接着跑，不在界面这边 +1 —— 算错会跳过或重跑一步
  const o = flowLastOutcome;
  if (o?.resumeFrom !== undefined) runFlow(o.resumeFrom);
};
$("flowNo").onclick = () => {
  flowConfirm.hidden = true;
  flowStatus.textContent = "停下了，前面几步的结果还在收件箱里";
};
$("copyFlowPrompt").onclick = async () => {
  try {
    flowPrompt ||= await invoke<string>("dock_flow_prompt");
    await writeClipboard(flowPrompt);
    flowStatus.textContent = "规范已复制 —— 把你的需求说给 AI，它给你一段 JSON";
  } catch (error) {
    flowStatus.textContent = String(error);
  }
};

/**
 * 「接到 AI」。
 *
 * 桥**没随包发出去**的时候要说实话。含糊地写「未配置」会让人以为是自己没设对，
 * 于是去翻文档、去改配置 —— 而真正的原因是那个文件根本不在机器上。
 */
async function renderMcp() {
  try {
    mcp = await invoke<McpInfo>("dock_mcp");
  } catch (error) {
    mcpState.textContent = String(error);
    return;
  }
  mcpSub.textContent = mcp.present
    ? `${mcp.tools} 个工具已就绪`
    : "桥不在这台机器上";
  mcpState.textContent = mcp.present
    ? `接上之后，AI 多出 ${mcp.tools} 件能做的事 —— 跟你在这儿点按钮能做的完全一样。`
    : "这个版本没带 MCP 桥。请装 0.2.0 以后的版本，或自己从源码构建。";
  mcpPath.textContent = mcp.path || "（找不到 podapp-mcp）";
  for (const id of ["copyClaude", "copyCodex"]) {
    ($(id) as HTMLButtonElement).disabled = !mcp.present;
  }
}

$("mcp").onclick = () => {
  setView("mcp", true);
  renderMcp();
};

async function copyMcp(which: "claude" | "codexToml", done: string) {
  if (!mcp) return;
  try {
    await writeClipboard(mcp[which]);
    mcpStatus.textContent = done;
  } catch (error) {
    mcpStatus.textContent = String(error);
  }
}
$("copyClaude").onclick = () =>
  copyMcp("claude", "已复制 —— 在终端里执行它，然后重开 Claude Code");
$("copyCodex").onclick = () =>
  copyMcp("codexToml", "已复制 —— 粘到 ~/.codex/config.toml 末尾，然后重开 Codex");

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
    const lower = path.toLowerCase();
    // 拖一张图集进来就装成宠物。现成宠物（Nyxie 那类）解压出来就是一张 webp，
    // 非要人先写一份 pet.json 才肯收，等于把门槛抬到没人愿意试。
    if (lower.endsWith(".png") || lower.endsWith(".webp")) {
      try {
        const it = await invoke<PetSummary>("dock_install_pet", { path });
        drop.textContent = `已装上宠物 ${it.displayName}`;
        await loadPets();
        selectPet(it.id);
        pet.once("jumping");
      } catch (error) {
        // 尺寸不对那类错误是**有用的信息**（要 1536×1872，这张是多少），
        // 原样让用户看到，别包成一句「安装失败」
        warn(error);
      }
      continue;
    }
    if (lower.endsWith(".flow.json")) {
      // 拖进来直接填好并验一遍 —— 让人少一步「复制粘贴」
      try {
        flowJson.value = await invoke<string>("dock_read_text", { path });
        setView("flow", true);
        await checkFlow();
      } catch (error) {
        warn(error);
      }
      continue;
    }
    if (!lower.endsWith(".pod")) continue;
    try {
      const pod = await invoke<PodInfo>("dock_install", { path });
      drop.textContent = `已安装 ${pod.name} v${pod.version}`;
      pet.once("jumping");
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

/**
 * 检查更新。
 *
 * **查不到就当没有，绝不打扰。** 更新端点有三个（国内域名在前、GitHub 兜底），
 * 但用户可能在裸网、代理坏了、或者三个都不通 —— 那是常态不是异常，
 * 弹个「检查更新失败」只会让人以为程序坏了。真有新版才让那个按钮出现。
 *
 * 也**不自动装**：浮舱是贴着 Codex 用的，装更新要重启，
 * 在用户正干活时自己重启是最招人烦的一种「贴心」。
 */
async function checkForUpdate() {
  const button = $("update") as HTMLButtonElement;
  // 类型**写出来**，别靠 `let update;` 让 TS 自己长出来。
  //
  // 自动推断（evolving any）在跨闭包引用时本来就靠不住，而它到底靠不靠得住
  // 取决于**整个模块**的控制流复杂度：这个文件长到一定程度，TS 就放弃分析，
  // 这里当场变成三个 TS7005/TS7034 错误。
  // 实测过：加宠物那几段之后就红了，去掉其中**任意一段**又绿 ——
  // 也就是说报错位置和真正的原因根本不在一起，下一个往这个文件里加代码的人
  // 会在一段跟更新毫不相干的代码上撞见它。写死类型，这条路直接堵上。
  let update: Awaited<ReturnType<typeof check>>;
  try {
    update = await check();
  } catch {
    return; // 网络不通 / 端点没上线 —— 静默
  }
  if (!update) return;
  // 收进 const 再给闭包用：`let` 上的判空 narrow 传不进闭包里去
  // （TS 没法保证按钮被点的那一刻它还是非空的）。
  const ready = update;

  button.textContent = `有新版 ${ready.version} · 点此更新`;
  button.hidden = false;
  button.onclick = async () => {
    button.disabled = true;
    try {
      // 进度只更新按钮上的字，不另开弹窗 —— 浮舱只有 380px 宽，弹窗会盖住全部内容
      let got = 0;
      await ready.downloadAndInstall((e) => {
        if (e.event === "Started") button.textContent = "正在下载…";
        else if (e.event === "Progress") {
          got += e.data.chunkLength;
          button.textContent = `正在下载… ${Math.round(got / 1024)} KB`;
        } else if (e.event === "Finished") button.textContent = "正在安装…";
      });
      await relaunch();
    } catch (error) {
      // 失败要说人话并且**留一条能自己走的路**，别只说「更新失败」
      button.textContent = "更新失败，点此手动下载";
      button.disabled = false;
      button.onclick = () => openUrl(RELEASES_URL).catch(warn);
      warn(error);
    }
  };
}

const RELEASES_URL = "https://github.com/dongsheng123132/podapp/releases/latest";

applySavedSkin();
loadPets();
restorePosition().then(refresh);
checkForUpdate();
