import boatSkin from "./skins/boat.dock-skin.json";
import catSkin from "./skins/cat.dock-skin.json";
import minimalSkin from "./skins/minimal.dock-skin.json";

export type DockSkin = {
  spec: "podapp/dock-skin@0.1";
  id: string;
  name: string;
  author: string;
  version: string;
  mark: string;
  colors: {
    background: string;
    surface: string;
    foreground: string;
    muted: string;
    border: string;
    accent: string;
    success: string;
    markBackground: string;
  };
  radius: number;
};

const SPEC = "podapp/dock-skin@0.1";
const ID = /^[a-z0-9][a-z0-9.-]{2,80}$/;
const VERSION = /^\d+\.\d+\.\d+(?:-[a-z0-9.-]+)?$/i;
const HEX = /^#[0-9a-f]{6}$/i;
const CUSTOM_SKINS_KEY = "podapp.custom-skins.v1";

function record(value: unknown, field: string): Record<string, unknown> {
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    throw new Error(`${field} 必须是对象`);
  }
  return value as Record<string, unknown>;
}

function text(
  value: unknown,
  field: string,
  maxLength: number,
  pattern?: RegExp,
): string {
  if (typeof value !== "string" || value.length === 0 || value.length > maxLength) {
    throw new Error(`${field} 必须是 1-${maxLength} 个字符`);
  }
  if (pattern && !pattern.test(value)) throw new Error(`${field} 格式不正确`);
  return value;
}

function color(value: unknown, field: string): string {
  return text(value, field, 7, HEX).toLowerCase();
}

export function parseSkin(value: unknown): DockSkin {
  const source = record(value, "皮肤");
  if (source.spec !== SPEC) throw new Error(`只支持 ${SPEC}`);
  const colors = record(source.colors, "colors");
  const mark = text(source.mark, "mark", 8);
  if (Array.from(mark).length > 4 || /[\u0000-\u001f]/.test(mark)) {
    throw new Error("mark 最多 4 个可见字符");
  }
  const radius = source.radius;
  if (typeof radius !== "number" || !Number.isFinite(radius) || radius < 0 || radius > 16) {
    throw new Error("radius 必须是 0-16 的数字");
  }

  return {
    spec: SPEC,
    id: text(source.id, "id", 81, ID),
    name: text(source.name, "name", 32),
    author: text(source.author, "author", 40),
    version: text(source.version, "version", 32, VERSION),
    mark,
    colors: {
      background: color(colors.background, "colors.background"),
      surface: color(colors.surface, "colors.surface"),
      foreground: color(colors.foreground, "colors.foreground"),
      muted: color(colors.muted, "colors.muted"),
      border: color(colors.border, "colors.border"),
      accent: color(colors.accent, "colors.accent"),
      success: color(colors.success, "colors.success"),
      markBackground: color(colors.markBackground, "colors.markBackground"),
    },
    radius: Math.round(radius),
  };
}

export const builtinSkins = [boatSkin, catSkin, minimalSkin].map(parseSkin);

export function loadCustomSkins(): DockSkin[] {
  try {
    const value = JSON.parse(localStorage.getItem(CUSTOM_SKINS_KEY) ?? "[]");
    return Array.isArray(value) ? value.flatMap((item) => {
      try {
        return [parseSkin(item)];
      } catch {
        return [];
      }
    }) : [];
  } catch {
    return [];
  }
}

export function saveCustomSkins(skins: DockSkin[]) {
  localStorage.setItem(CUSTOM_SKINS_KEY, JSON.stringify(skins));
}

export function applySkin(skin: DockSkin) {
  const style = document.documentElement.style;
  style.setProperty("--bg", skin.colors.background);
  style.setProperty("--surface", skin.colors.surface);
  style.setProperty("--fg", skin.colors.foreground);
  style.setProperty("--dim", skin.colors.muted);
  style.setProperty("--line", skin.colors.border);
  style.setProperty("--accent", skin.colors.accent);
  style.setProperty("--success", skin.colors.success);
  style.setProperty("--mark-bg", skin.colors.markBackground);
  style.setProperty("--radius", `${skin.radius}px`);
}
