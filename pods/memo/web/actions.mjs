// 备忘贴动作核心。这里不碰 DOM，GUI、CLI 和 AI 无头调用共用这一份实现。
import { ACTION, defineActions } from "./action-parity.generated.mjs";

const STORAGE_KEY = "notes";
const COLORS = new Set(["yellow", "green", "blue", "rose"]);

function cleanNote(value) {
  if (!value || typeof value !== "object" || !value.id) return null;
  return {
    id: String(value.id).slice(0, 120),
    title: String(value.title ?? "").slice(0, 120),
    body: String(value.body ?? "").slice(0, 20000),
    color: COLORS.has(value.color) ? value.color : "yellow",
    pinned: Boolean(value.pinned),
    created_at: String(value.created_at ?? ""),
    updated_at: String(value.updated_at ?? ""),
  };
}

function sortNotes(notes) {
  return notes.sort((a, b) =>
    Number(b.pinned) - Number(a.pinned) ||
    b.updated_at.localeCompare(a.updated_at),
  );
}

async function readNotes(pod) {
  const stored = await pod.storage.get(STORAGE_KEY);
  return sortNotes((Array.isArray(stored) ? stored : []).map(cleanNote).filter(Boolean));
}

async function writeNotes(pod, notes) {
  await pod.storage.set(STORAGE_KEY, sortNotes(notes));
}

function noteId() {
  return `note-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 8)}`;
}

export default defineActions({
  [ACTION.APP_MEMO_NOTE_LIST]: async (input, ctx) => {
    const query = String(input.query ?? "").trim().toLocaleLowerCase();
    const all = await readNotes(ctx.pod);
    const notes = query
      ? all.filter((note) => `${note.title}\n${note.body}`.toLocaleLowerCase().includes(query))
      : all;
    return { ok: true, count: notes.length, notes, message: `找到 ${notes.length} 条备忘` };
  },

  [ACTION.APP_MEMO_NOTE_SAVE]: async (input, ctx) => {
    const notes = await readNotes(ctx.pod);
    const requestedId = String(input.id ?? "").trim();
    const existing = requestedId ? notes.find((note) => note.id === requestedId) : null;
    const now = new Date().toISOString();
    const note = {
      id: (existing?.id ?? requestedId) || noteId(),
      title: String(input.title ?? existing?.title ?? "").slice(0, 120),
      body: String(input.body ?? existing?.body ?? "").slice(0, 20000),
      color: COLORS.has(input.color) ? input.color : (existing?.color ?? "yellow"),
      pinned: input.pinned === undefined ? Boolean(existing?.pinned) : Boolean(input.pinned),
      created_at: existing?.created_at || now,
      updated_at: now,
    };
    const next = notes.filter((item) => item.id !== note.id);
    next.push(note);
    await writeNotes(ctx.pod, next);
    return { ok: true, note, count: next.length, message: existing ? "备忘已更新" : "备忘已保存" };
  },

  [ACTION.APP_MEMO_NOTE_REMOVE]: async (input, ctx) => {
    const id = String(input.id ?? "").trim();
    if (!id) throw new Error("要删除哪一条？请提供备忘 id");
    const notes = await readNotes(ctx.pod);
    const next = notes.filter((note) => note.id !== id);
    await writeNotes(ctx.pod, next);
    const removed = next.length !== notes.length;
    return {
      ok: true,
      removed,
      count: next.length,
      message: removed ? "备忘已删除" : "这条备忘已经不存在",
    };
  },
});

export { cleanNote, sortNotes };
