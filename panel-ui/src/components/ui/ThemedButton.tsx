import React from "react";

interface ThemedButtonProps extends React.ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: "primary" | "secondary" | "ghost" | "pill";
  icon?: React.ReactNode;
  children?: React.ReactNode;
  active?: boolean;
}

export const ThemedButton: React.FC<ThemedButtonProps> = ({
  variant = "secondary",
  icon,
  children,
  active = false,
  className = "",
  ...props
}) => {
  const baseClasses =
    "inline-flex items-center justify-center gap-2 rounded-lg text-xs font-medium transition-all duration-150 press-attenuation";

  const variants = {
    primary:
      "bg-blue-600 hover:bg-blue-500 text-white font-semibold shadow-sm border border-blue-400/30 px-3.5 py-2",
    secondary:
      "bg-[#1b1c20] hover:bg-[#23252a] text-white/90 border border-white/[0.08] hover:border-white/20 px-3 py-1.5",
    ghost:
      "bg-transparent hover:bg-white/[0.05] text-white/70 hover:text-white px-2.5 py-1.5",
    pill: `rounded-full px-3 py-1 border border-white/10 ${
      active
        ? "bg-white/10 text-white border-white/25"
        : "bg-transparent text-white/60 hover:text-white hover:bg-white/5"
    }`,
  };

  return (
    <button
      type="button"
      className={`${baseClasses} ${variants[variant]} ${className}`}
      {...props}
    >
      {icon && <span className="shrink-0">{icon}</span>}
      {children}
    </button>
  );
};

export default ThemedButton;
