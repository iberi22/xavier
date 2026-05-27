// Xavier Generative UI React Wrapper
// Bridges vanilla JS renderer into React panel-ui
import { useEffect, useRef } from "react";
import { XavierUIRenderer as RendererClass } from "../generative-ui/renderer.js";

interface XavierUIRendererProps {
  xuiJson: string | Record<string, unknown>;
  onAction?: (event: Record<string, unknown>) => void;
  onSubmit?: (event: Record<string, unknown>) => void;
}

export function XavierUIRenderer({
  xuiJson,
  onAction,
  onSubmit,
}: XavierUIRendererProps) {
  const containerRef = useRef<HTMLDivElement>(null);
  const rendererRef = useRef<RendererClass | null>(null);

  useEffect(() => {
    if (!containerRef.current) {
      return;
    }

    // Generate unique container id
    const containerId = `xui-render-${Math.random().toString(36).slice(2, 9)}`;
    containerRef.current.id = containerId;

    // Initialize renderer
    const renderer = new RendererClass(containerId, {
      theme: "dark",
      onAction: (event: Record<string, unknown>) => {
        console.log("[XavierUI] Action:", event);
        onAction?.(event);
      },
      onSubmit: (event: Record<string, unknown>) => {
        console.log("[XavierUI] Submit:", event);
        onSubmit?.(event);
      },
    });

    rendererRef.current = renderer;

    // Render the JSON
    renderer.render(xuiJson);

    return () => {
      if (containerRef.current) {
        containerRef.current.innerHTML = "";
      }
      rendererRef.current = null;
    };
  }, [xuiJson, onAction, onSubmit]);

  return (
    <div className="xui-render-surface">
      <div className="xui-render-header">
        <span className="xui-render-title">Xavier UI Render Surface</span>
        <span className="xui-render-mode">JSON Schema</span>
      </div>
      <div ref={containerRef} className="xui-render-container" />
    </div>
  );
}

export default XavierUIRenderer;
