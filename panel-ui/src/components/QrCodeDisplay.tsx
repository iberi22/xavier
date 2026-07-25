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
      // Remove any script elements
      if (element.tagName.toLowerCase() === "script") {
         element.remove();
         return;
      }

      // Remove foreignObject as it can contain unconstrained HTML
      if (element.tagName.toLowerCase() === "foreignobject") {
         element.remove();
         return;
      }

      const attributes = Array.from(element.attributes);
      attributes.forEach((attr) => {
        const name = attr.name.toLowerCase();
        const value = attr.value.toLowerCase().trim();
        // Strip control characters and whitespaces that can bypass naive startsWith checks
        const strippedValue = value.replace(/[\u0000-\u0020]/g, "");

        // Remove event handlers
        if (name.startsWith("on")) {
          element.removeAttribute(attr.name);
        }

        // Remove javascript: URIs in any attribute (like href or xlink:href)
        if (strippedValue.startsWith("javascript:")) {
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
