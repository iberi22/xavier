import React from "react";
import type { LucideIcon } from "lucide-react";

export type BorderedIconVariant = "default" | "subtle" | "primary" | "ghost";
export type BorderedIconSize = "sm" | "md" | "lg";
export type BorderedIconRounded = "sm" | "md" | "lg" | "full";

export interface BorderedIconProps extends React.HTMLAttributes<HTMLElement> {
  icon?: LucideIcon;
  children?: React.ReactNode;
  variant?: BorderedIconVariant;
  size?: BorderedIconSize;
  rounded?: BorderedIconRounded;
  interactive?: boolean;
  disabled?: boolean;
  active?: boolean;
  ariaLabel?: string;
  className?: string;
}

const sizeClasses: Record<BorderedIconSize, { container: string; icon: string }> = {
  sm: {
    container: "p-1.5 text-xs w-7 h-7",
    icon: "w-3.5 h-3.5",
  },
  md: {
    container: "p-2 text-sm w-9 h-9",
    icon: "w-4 h-4",
  },
  lg: {
    container: "p-2.5 text-base w-11 h-11",
    icon: "w-5 h-5",
  },
};

const roundedClasses: Record<BorderedIconRounded, string> = {
  sm: "rounded",
  md: "rounded-md",
  lg: "rounded-lg",
  full: "rounded-full",
};

const variantClasses: Record<BorderedIconVariant, string> = {
  default:
    "border border-zinc-200 dark:border-white/10 bg-zinc-50/50 dark:bg-white/5 text-zinc-700 dark:text-zinc-200 hover:bg-zinc-100 dark:hover:bg-white/10 hover:border-zinc-300 dark:hover:border-white/20",
  subtle:
    "border border-zinc-200/60 dark:border-white/5 bg-transparent text-zinc-600 dark:text-zinc-400 hover:bg-zinc-100/80 dark:hover:bg-white/5 hover:text-zinc-900 dark:hover:text-zinc-100",
  primary:
    "border border-emerald-500/30 dark:border-emerald-500/40 bg-emerald-500/10 dark:bg-emerald-500/20 text-emerald-600 dark:text-emerald-400 hover:bg-emerald-500/20 dark:hover:bg-emerald-500/30",
  ghost:
    "border border-transparent bg-transparent text-zinc-600 dark:text-zinc-400 hover:bg-zinc-100 dark:hover:bg-white/10 hover:text-zinc-900 dark:hover:text-zinc-100",
};

export const BorderedIcon: React.FC<BorderedIconProps> = ({
  icon: Icon,
  children,
  variant = "default",
  size = "md",
  rounded = "rounded-lg" in roundedClasses ? "rounded-lg" : "lg",
  interactive,
  disabled = false,
  active = false,
  ariaLabel,
  className = "",
  onClick,
  ...restProps
}) => {
  const isInteractive = interactive ?? Boolean(onClick);
  const activeClass = active
    ? "ring-2 ring-emerald-500/50 border-emerald-500/80 dark:border-emerald-400/80"
    : "";
  const disabledClass = disabled
    ? "opacity-50 pointer-events-none cursor-not-allowed"
    : "";

  // Built-in press scale attenuation micro-interaction feedback
  const pressAttenuationClass = isInteractive && !disabled
    ? "cursor-pointer transition-all duration-150 ease-out active:scale-95 press-attenuation select-none"
    : "";

  const containerClasses = [
    "inline-flex items-center justify-center font-medium shadow-xs focus:outline-none focus-visible:ring-2 focus-visible:ring-emerald-500 focus-visible:ring-offset-2 dark:focus-visible:ring-offset-zinc-950",
    sizeClasses[size].container,
    roundedClasses[rounded],
    variantClasses[variant],
    activeClass,
    disabledClass,
    pressAttenuationClass,
    className,
  ]
    .filter(Boolean)
    .join(" ");

  const renderedIcon = Icon ? (
    <Icon className={sizeClasses[size].icon} aria-hidden="true" />
  ) : (
    children
  );

  if (isInteractive) {
    return (
      <button
        type="button"
        disabled={disabled}
        onClick={onClick}
        aria-label={ariaLabel}
        aria-pressed={active}
        className={containerClasses}
        {...(restProps as React.ButtonHTMLAttributes<HTMLButtonElement>)}
      >
        {renderedIcon}
      </button>
    );
  }

  return (
    <div
      role="img"
      aria-label={ariaLabel}
      className={containerClasses}
      {...restProps}
    >
      {renderedIcon}
    </div>
  );
};

export default BorderedIcon;
