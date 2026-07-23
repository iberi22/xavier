import type React from "react";
import { useMemo } from "react";

interface QrCodeDisplayProps {
  svg?: string;
  value?: string;
}

const sanitizeSvg = (svgString: string): string => {
  if (!svgString) return "";
  try {
    const parser = new DOMParser();
    const doc = parser.parseFromString(svgString, "image/svg+xml");

    // Check for parsing errors
    if (doc.querySelector("parsererror")) {
      return "";
    }

    const root = doc.documentElement;
    if (root.tagName.toLowerCase() !== "svg") {
      return "";
    }

    // Recursively check and remove elements/attributes
    const removeDangerousNodes = (element: Element) => {
      const tagName = element.tagName.toLowerCase();
      // Remove any script elements
      if (tagName === "script") {
         element.remove();
         return;
      }

      // Remove foreignObject as it can contain unconstrained HTML
      if (tagName === "foreignobject") {
         element.remove();
         return;
      }

      // Remove animation tags which can be used to bypass attribute checks
      if (["animate", "set", "animatemotion", "animatetransform", "mpath", "animatecolor"].includes(tagName)) {
         element.remove();
         return;
      }

      const attributes = Array.from(element.attributes);
      attributes.forEach((attr) => {
        const name = attr.name.toLowerCase();
        // Strip control characters to prevent bypasses like java\x09script:
        const value = attr.value.toLowerCase().replace(/[\x00-\x20\x7F-\x9F]/g, "");

        // Remove event handlers
        if (name.startsWith("on")) {
          element.removeAttribute(attr.name);
        }

        // Remove javascript: URIs in any attribute (like href or xlink:href)
        if (value.startsWith("javascript:")) {
           element.removeAttribute(attr.name);
        }
      });

      Array.from(element.children).forEach(removeDangerousNodes);
    };

    removeDangerousNodes(root);

    return root.outerHTML;
  } catch (error) {
    console.error("Error sanitizing SVG:", error);
    return "";
  }
};

export const QrCodeDisplay: React.FC<QrCodeDisplayProps> = ({ svg, value }) => {
  const sanitizedSvg = useMemo(() => (svg ? sanitizeSvg(svg) : ""), [svg]);

  return (
    <div className="bg-white p-4 rounded-xl inline-block mx-auto">
      {sanitizedSvg ? (
        <div
          // biome-ignore lint/security/noDangerouslySetInnerHtml: Sanitized correctly in sanitizeSvg using DOMParser to avoid XSS
          dangerouslySetInnerHTML={{ __html: sanitizedSvg }}
          className="w-48 h-48"
        />
      ) : (
        <div className="w-48 h-48 bg-gray-200 flex items-center justify-center text-black text-xs text-center p-4">
          {value || "QR Code Placeholder"}
        </div>
      )}
    </div>
  );
};
