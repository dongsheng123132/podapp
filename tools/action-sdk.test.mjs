import assert from "node:assert/strict";
import test from "node:test";
import {
  ACTION,
  ACTION_IDS,
  defineActions,
} from "../pods/memo/web/action-parity.generated.mjs";

const complete = () => Object.fromEntries(ACTION_IDS.map((id) => [id, async () => ({ ok: true })]));

test("generated Action constants preserve every manifest ID", () => {
  assert.deepEqual(new Set(Object.values(ACTION)), new Set(ACTION_IDS));
  assert.equal(ACTION.APP_MEMO_NOTE_LIST, "app.memo.note.list");
});

test("a missing handler fails before a GUI or agent can call it", () => {
  const handlers = complete();
  delete handlers[ACTION.APP_MEMO_NOTE_SAVE];
  assert.throws(() => defineActions(handlers), /missing=app\.memo\.note\.save/);
});

test("an undeclared handler fails instead of becoming a shadow action", () => {
  assert.throws(
    () => defineActions({ ...complete(), "app.memo.note.ghost": async () => ({ ok: true }) }),
    /unknown=app\.memo\.note\.ghost/,
  );
});

test("a non-function handler fails during module loading", () => {
  assert.throws(
    () => defineActions({ ...complete(), [ACTION.APP_MEMO_NOTE_REMOVE]: null }),
    /not_functions=app\.memo\.note\.remove/,
  );
});
