import { useCallback, useMemo, useState, type CSSProperties } from "react";
import defaultUi from "./maloca.ui.json";
import type { MalocaSectionId, MalocaUiConfig } from "./types";

const STORAGE_KEY = "xavier.maloca.ui.v1";

function loadConfig(): MalocaUiConfig {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (raw) {
      const parsed = JSON.parse(raw) as MalocaUiConfig;
      if (parsed?.version && parsed.theme && parsed.layout) return parsed;
    }
  } catch {
    /* ignore */
  }
  return defaultUi as MalocaUiConfig;
}

export function useMalocaUi() {
  const [config, setConfig] = useState<MalocaUiConfig>(loadConfig);
  const [editLayout, setEditLayout] = useState(false);

  const persist = useCallback((next: MalocaUiConfig) => {
    setConfig(next);
    localStorage.setItem(STORAGE_KEY, JSON.stringify(next));
  }, []);

  const themeStyle = useMemo(() => {
    const t = config.theme;
    return {
      ["--maloca-bg" as string]: t.bg,
      ["--maloca-bg-elevated" as string]: t.bgElevated,
      ["--maloca-bg-muted" as string]: t.bgMuted,
      ["--maloca-text" as string]: t.text,
      ["--maloca-text-muted" as string]: t.textMuted,
      ["--maloca-accent" as string]: t.accent,
      ["--maloca-accent-soft" as string]: t.accentSoft,
      ["--maloca-border" as string]: t.border,
      ["--maloca-warning" as string]: t.warning,
      ["--maloca-warning-bg" as string]: t.warningBg,
      ["--maloca-danger" as string]: t.danger,
      ["--maloca-radius" as string]: t.radius,
      ["--maloca-space" as string]: t.space,
      ["--maloca-font" as string]: t.font,
      ["--maloca-font-mono" as string]: t.fontMono,
    } as CSSProperties;
  }, [config.theme]);

  const sections = useMemo(
    () => config.layout.sections.filter((s) => s.enabled),
    [config.layout.sections],
  );

  const moveSection = useCallback(
    (id: MalocaSectionId, dir: -1 | 1) => {
      if (!config.editMode.allowReorder) return;
      const list = [...config.layout.sections];
      const idx = list.findIndex((s) => s.id === id);
      const next = idx + dir;
      if (idx < 0 || next < 0 || next >= list.length) return;
      [list[idx], list[next]] = [list[next], list[idx]];
      persist({ ...config, layout: { sections: list } });
    },
    [config, persist],
  );

  const resetUi = useCallback(() => {
    localStorage.removeItem(STORAGE_KEY);
    setConfig(defaultUi as MalocaUiConfig);
  }, []);

  const softAccent = useCallback(() => {
    if (!config.editMode.allowThemeEdit) return;
    persist({
      ...config,
      theme: {
        ...config.theme,
        accent: "#6b8578",
        accentSoft: "#d2ddd5",
        bg: "#f7f8f5",
      },
    });
  }, [config, persist]);

  return {
    config,
    themeStyle,
    sections,
    editLayout,
    setEditLayout,
    moveSection,
    resetUi,
    softAccent,
  };
}
