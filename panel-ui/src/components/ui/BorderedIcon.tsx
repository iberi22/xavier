import React from "react";

interface BorderedIconProps extends React.HTMLAttributes<HTMLDivElement> {
  children: React.ReactNode;
  size?: "sm" | "md" | "lg";
  active?: boolean;
  interactive?: boolean;
}

export const BorderedIcon: React.FC<BorderedIconProps> = ({
  children,
  size = "md",
  active = false,
  interactive = true,
  className = "",
  ...props
}) => {
  const sizeClasses = {
    sm: "w-7 h-7 p-1 text-xs",
    md: "w-9 h-9 p-1.5 text-sm",
    lg: "w-11 h-11 p-2.5 text-base",
  };

  return (
    <div
      className={`
        inline-flex items-center justify-center rounded-lg border transition-all duration-150
        ${sizeClasses[size]}
        ${
          active
            ? "border-blue-500/50 bg-blue-500/10 text-blue-400"
            : "border-white/10 dark:border-white/10 bg-white/[0.02] text-foreground/80 hover:border-white/20 dark:hover:border-white/25"
        }
        ${interactive ? "cursor-pointer press-attenuation hover-attenuation" : ""}
        ${className}
      `}
      {...props}
    >
      {children}
    </div>
  );
};

export default BorderedIcon;
