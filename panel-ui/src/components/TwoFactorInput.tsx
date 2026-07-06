import React from "react";

interface TwoFactorInputProps {
  value: string;
  onChange: (value: string) => void;
}

export const TwoFactorInput: React.FC<TwoFactorInputProps> = ({ value, onChange }) => {
  const handleChange = (e: React.ChangeEvent<HTMLInputElement>) => {
    const val = e.target.value.replace(/\D/g, "").slice(0, 6);
    onChange(val);
  };

  return (
    <div className="flex gap-2 justify-center">
      {Array.from({ length: 6 }).map((_, i) => (
        <div
          key={i}
          className={`w-10 h-12 border ${
            value.length === i ? "border-[#39ff14]" : "border-white/10"
          } bg-white/5 rounded-lg flex items-center justify-center text-lg font-bold text-[#39ff14]`}
        >
          {value[i] || ""}
        </div>
      ))}
      <input
        type="text"
        value={value}
        onChange={handleChange}
        className="absolute opacity-0 w-0 h-0"
        autoFocus
        maxLength={6}
      />
    </div>
  );
};
