import { vi } from "vitest";

vi.mock("@openuidev/react-ui", () => ({
  createTheme: vi.fn(() => ({})),
  designTokens: {
    color: {
      info: "#3B82F6",
      success: "#19C37D",
      muted: "#5D584B",
    },
  },
}));

vi.mock("../src/theme", () => ({
  designTokens: {
    color: {
      bg: "#F6F3EA",
      surface: "#FFFDF8",
      surface2: "#EDE7D8",
      text: "#111111",
      border: "#000000",
      accent: "#F2F230",
      accentStrong: "#D8D81E",
      danger: "#FF5C5C",
      info: "#3B82F6",
      success: "#19C37D",
      muted: "#5D584B",
    },
  },
}));

import "@testing-library/jest-dom";

// Global define for Vitest environment
(globalThis as any).__APP_VERSION__ = "0.10.0";
