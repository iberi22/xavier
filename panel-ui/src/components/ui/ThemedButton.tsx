import React from "react";
import type { LucideIcon } from "lucide-react";
import { BorderedIcon } from "./BorderedIcon";
import { LoadingSpinner } from "./LoadingSpinner";

export type ThemedButtonVariant = "primary" | "secondary" | "ghost" | "subtle";
export type ThemedButtonSize = "sm" | "md" | "lg";

export interface ThemedButtonProps extends React.ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: ThemedButtonVariant;
  size?: ThemedButtonSize;
  icon?: LucideIcon;
  iconPosition?: "left" | "right";
  iconBordered?: boolean;
  isLoading?: boolean;
  active?: boolean;
  children?: React.ReactNode;
}

const sizeClasses: Record<ThemedButtonSize, string> = {
  sm: "px-2.5 py-1 text-xs min-h-[30px] rounded-md gap-1.5",
  md: "px-3.5 py-1.5 text-sm min-h-[36px] rounded-lg gap-2",
  lg: "px-4.5 py-2 text-base min-h-[42px] rounded-xl gap-2.5",
};

const iconSizeClasses: Record<ThemedButtonSize, string> = {
  sm: "w-3.5 h-3.5",
  md: "w-4 h-4",
  lg: "w-5 h-5",
};

const variantClasses: Record<ThemedButtonVariant, string> = {
  primary:
    "bg-emerald-600 hover:bg-emerald-500 active:bg-emerald-700 text-white border border-emerald-500/40 shadow-xs focus-visible:ring-emerald-500",
  secondary:
    "bg-zinc-100 dark:bg-zinc-800 hover:bg-zinc-200 dark:hover:bg-zinc-700 text-zinc-900 dark:text-zinc-100 border border-zinc-200 dark:border-white/10 shadow-xs focus-visible:ring-emerald-500",
  subtle:
    "bg-zinc-50 dark:bg-white/5 hover:bg-zinc-100 dark:hover:bg-white/10 text-zinc-700 dark:text-zinc-300 border border-zinc-200/80 dark:border-white/10 focus-visible:ring-emerald-500",
  ghost:
    "bg-transparent hover:bg-zinc-100 dark:hover:bg-white/10 text-zinc-700 dark:text-zinc-300 border border-transparent focus-visible:ring-emerald-500",
};

export const ThemedButton: React.FC<ThemedButtonProps> = ({
  type = "button",
  variant = "secondary",
  size = "md",
  icon: Icon,
  iconPosition = "left",
  iconBordered = false,
  isLoading = false,
  active = false,
  disabled = false,
  className = "",
  children,
  ...restProps
}) => {
  const isDisabled = disabled || isLoading;

  const renderIcon = () => {
    if (isLoading) {
      return (
        <span aria-hidden="true" className="shrink-0 inline-flex items-center justify-center">
          <LoadingSpinner className="shrink-0" />
        </span>
      );
    }

    if (!Icon) return null;

    if (iconBordered) {
      return (
        <BorderedIcon
          icon={Icon}
          size={size === "lg" ? "md" : "sm"}
          variant={variant === "primary" ? "primary" : "subtle"}
          interactive={false}
          className="shrink-0"
        />
      );
    }

    return <Icon className={`${iconSizeClasses[size]} shrink-0`} aria-hidden="true" />;
  };

  const activeClass = active
    ? "ring-2 ring-emerald-500/50 border-emerald-500/80 dark:border-emerald-400/80"
    : "";

  const disabledClass = isDisabled
    ? "opacity-50 pointer-events-none cursor-not-allowed"
    : "cursor-pointer active:scale-[0.98] transition-transform duration-150 ease-out press-attenuation";

  const buttonClasses = [
    "inline-flex items-center justify-center font-medium select-none focus:outline-none focus-visible:ring-2 focus-visible:ring-offset-2 dark:focus-visible:ring-offset-zinc-950",
    sizeClasses[size],
    variantClasses[variant],
    activeClass,
    disabledClass,
    className,
  ]
    .filter(Boolean)
    .join(" ");

  return (
    <button
      type={type}
      disabled={isDisabled}
      aria-pressed={active}
      aria-disabled={isDisabled}
      className={buttonClasses}
      {...restProps}
    >
      {iconPosition === "left" && renderIcon()}
      {children && <span>{children}</span>}
      {iconPosition === "right" && renderIcon()}
    </button>
  );
};

export default ThemedButton;
