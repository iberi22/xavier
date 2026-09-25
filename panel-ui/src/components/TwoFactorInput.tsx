import type React from "react";

interface TwoFactorInputProps {
  value: string;
  onChange: (value: string) => void;
  /**
   * Max characters accepted. Default 6 fits a live TOTP code (src/auth2/mod.rs
   * `build_totp`: 6 digits). LoginPage passes 8 because `/auth/login` also
   * accepts a one-shot 8-digit backup code in the same field (see the
   * `consume_backup_code` fallback in `login_handler`) — without this, typing
   * a backup code here silently truncated to 6 digits and could never match.
   */
  length?: number;
}

export const TwoFactorInput: React.FC<TwoFactorInputProps> = ({
  value,
  onChange,
  length = 6,
}) => {
  const handleChange = (e: React.ChangeEvent<HTMLInputElement>) => {
    const val = e.target.value.replace(/\D/g, "").slice(0, length);
    onChange(val);
  };

  return (
    <div className="flex gap-2 justify-center flex-wrap">
      {Array.from({ length }).map((_, i) => (
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
        maxLength={length}
      />
    </div>
  );
};
