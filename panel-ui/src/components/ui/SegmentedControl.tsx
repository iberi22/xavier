import React, { useRef } from "react";
import type { LucideIcon } from "lucide-react";

export interface SegmentedControlOption<T extends string = string> {
  value: T;
  label: React.ReactNode;
  icon?: LucideIcon;
  badge?: React.ReactNode;
  disabled?: boolean;
  ariaLabel?: string;
}

export type SegmentedControlSize = "sm" | "md" | "lg";

export interface SegmentedControlProps<T extends string = string> {
  options: SegmentedControlOption<T>[];
  value: T;
  onChange: (value: T) => void;
  size?: SegmentedControlSize;
  disabled?: boolean;
  className?: string;
  ariaLabel?: string;
  name?: string;
}

const containerSizeClasses: Record<SegmentedControlSize, string> = {
  sm: "p-0.5 gap-0.5 text-xs rounded-lg",
  md: "p-1 gap-1 text-sm rounded-xl",
  lg: "p-1.5 gap-1.5 text-base rounded-2xl",
};

const itemSizeClasses: Record<SegmentedControlSize, string> = {
  sm: "px-2.5 py-1 min-h-[28px]",
  md: "px-3.5 py-1.5 min-h-[34px]",
  lg: "px-4 py-2 min-h-[40px]",
};

const iconSizeClasses: Record<SegmentedControlSize, string> = {
  sm: "w-3.5 h-3.5",
  md: "w-4 h-4",
  lg: "w-5 h-5",
};

export function SegmentedControl<T extends string = string>({
  options,
  value,
  onChange,
  size = "md",
  disabled = false,
  className = "",
  ariaLabel,
  name,
}: SegmentedControlProps<T>): React.ReactElement {
  const containerRef = useRef<HTMLDivElement>(null);

  const enabledOptions = options.filter((opt) => !opt.disabled && !disabled);

  const handleKeyDown = (e: React.KeyboardEvent<HTMLButtonElement>, currentIndex: number) => {
    if (disabled || enabledOptions.length === 0) return;

    let targetIndex = -1;

    switch (e.key) {
      case "ArrowLeft":
      case "ArrowUp": {
        e.preventDefault();
        targetIndex = (currentIndex - 1 + options.length) % options.length;
        while (options[targetIndex]?.disabled && targetIndex !== currentIndex) {
          targetIndex = (targetIndex - 1 + options.length) % options.length;
        }
        break;
      }
      case "ArrowRight":
      case "ArrowDown": {
        e.preventDefault();
        targetIndex = (currentIndex + 1) % options.length;
        while (options[targetIndex]?.disabled && targetIndex !== currentIndex) {
          targetIndex = (targetIndex + 1) % options.length;
        }
        break;
      }
      case "Home": {
        e.preventDefault();
        targetIndex = options.findIndex((opt) => !opt.disabled);
        break;
      }
      case "End": {
        e.preventDefault();
        for (let i = options.length - 1; i >= 0; i--) {
          if (!options[i].disabled) {
            targetIndex = i;
            break;
          }
        }
        break;
      }
      default:
        break;
    }

    if (targetIndex !== -1 && !options[targetIndex]?.disabled) {
      const nextOption = options[targetIndex];
      onChange(nextOption.value);
      const buttons = containerRef.current?.querySelectorAll<HTMLButtonElement>('[role="radio"]');
      buttons?.[targetIndex]?.focus();
    }
  };

  return (
    <div
      ref={containerRef}
      role="radiogroup"
      aria-label={ariaLabel}
      aria-disabled={disabled}
      data-name={name}
      className={[
        "inline-flex items-center select-none bg-zinc-100 dark:bg-zinc-900/90 border border-zinc-200 dark:border-white/10 backdrop-blur-sm",
        containerSizeClasses[size],
        disabled ? "opacity-50 pointer-events-none" : "",
        className,
      ]
        .filter(Boolean)
        .join(" ")}
    >
      {options.map((option, index) => {
        const isSelected = value === option.value;
        const isDisabled = disabled || option.disabled;
        const Icon = option.icon;

        return (
          <button
            key={option.value}
            type="button"
            role="radio"
            aria-checked={isSelected}
            aria-label={option.ariaLabel || (typeof option.label === "string" ? option.label : undefined)}
            disabled={isDisabled}
            tabIndex={isSelected ? 0 : -1}
            onClick={() => {
              if (!isDisabled && !isSelected) {
                onChange(option.value);
              }
            }}
            onKeyDown={(e) => handleKeyDown(e, index)}
            className={[
              "relative inline-flex items-center justify-center gap-2 font-medium rounded-lg transition-all duration-150 ease-out focus:outline-none focus-visible:ring-2 focus-visible:ring-emerald-500 focus-visible:ring-offset-1 dark:focus-visible:ring-offset-zinc-900",
              itemSizeClasses[size],
              isSelected
                ? "bg-white dark:bg-zinc-800 text-zinc-900 dark:text-zinc-100 shadow-xs border border-zinc-200/80 dark:border-white/15"
                : "text-zinc-600 dark:text-zinc-400 hover:text-zinc-900 dark:hover:text-zinc-200 hover:bg-zinc-200/50 dark:hover:bg-white/5 border border-transparent",
              isDisabled ? "opacity-40 cursor-not-allowed" : "cursor-pointer active:scale-95 press-attenuation",
            ]
              .filter(Boolean)
              .join(" ")}
          >
            {Icon && <Icon className={iconSizeClasses[size]} aria-hidden="true" />}
            <span>{option.label}</span>
            {option.badge && (
              <span className="ml-0.5 inline-flex items-center">{option.badge}</span>
            )}
          </button>
        );
      })}
    </div>
  );
}

export default SegmentedControl;
