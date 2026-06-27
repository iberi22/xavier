import React, { useState } from "react";
import { Eye, EyeOff } from "lucide-react";

interface PasswordInputProps extends React.InputHTMLAttributes<HTMLInputElement> {
  label?: string;
}

export const PasswordInput: React.FC<PasswordInputProps> = ({ label, ...props }) => {
  const [show, setShow] = useState(false);

  return (
    <div className="flex flex-col gap-1 w-full">
      {label && <label htmlFor={props.id} className="text-xs text-white/60 uppercase tracking-widest">{label}</label>}
      <div className="relative">
        <input
          {...props}
          type={show ? "text" : "password"}
          className={`w-full bg-white/5 border border-white/10 rounded-lg p-3 text-sm focus:border-[#39ff14] focus:outline-none transition-colors font-mono ${props.className || ""}`}
        />
        <button
          type="button"
          onClick={() => setShow(!show)}
          className="absolute right-3 top-1/2 -translate-y-1/2 text-white/40 hover:text-white/80 transition-colors"
        >
          {show ? <EyeOff size={18} /> : <Eye size={18} />}
        </button>
      </div>
    </div>
  );
};
