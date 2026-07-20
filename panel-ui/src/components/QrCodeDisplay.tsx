import type React from "react";

interface QrCodeDisplayProps {
  svg?: string;
  value?: string;
}

export const QrCodeDisplay: React.FC<QrCodeDisplayProps> = ({ svg, value }) => {
  return (
    <div className="bg-white p-4 rounded-xl inline-block mx-auto">
      {svg ? (
        <div dangerouslySetInnerHTML={{ __html: svg }} className="w-48 h-48" />
      ) : (
        <div className="w-48 h-48 bg-gray-200 flex items-center justify-center text-black text-xs text-center p-4">
          {value || "QR Code Placeholder"}
        </div>
      )}
    </div>
  );
};
