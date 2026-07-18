import { vi } from "vitest";
import React from "react";

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

// Globally mock react-force-graph-2d for JSDOM unit tests using pure React.createElement to prevent syntax errors in pure .ts files
vi.mock("react-force-graph-2d", () => {
  const DummyGraph = React.forwardRef(({ graphData, onNodeClick }: any, ref: any) => {
    const buttons = graphData?.nodes?.map((node: any) => {
      return React.createElement(
        "button",
        {
          key: node.id,
          "data-testid": `node-${node.id}`,
          onClick: (e: any) => {
            e.stopPropagation();
            if (onNodeClick) onNodeClick(node);
          },
        },
        node.label || node.id
      );
    }) || [];

    return React.createElement(
      "div",
      { "data-testid": "force-graph-mock" },
      ...buttons
    );
  });

  return {
    default: DummyGraph,
    __esModule: true,
  };
});

// Globally mock motion/react using pure React.createElement to disable transition delays in JSDOM
vi.mock("motion/react", () => {
  const dummy = React.forwardRef(({ children, ...props }: any, ref: any) => {
    const { exit, animate, initial, transition, layoutId, ...rest } = props;
    return React.createElement("div", { ref, ...rest }, children);
  });
  return {
    motion: {
      div: dummy,
      button: dummy,
      span: dummy,
      svg: dummy,
      path: dummy,
    },
    AnimatePresence: ({ children }: any) => React.createElement(React.Fragment, null, children),
  };
});

import "@testing-library/jest-dom";
