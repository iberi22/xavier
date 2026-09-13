import React from "react";

export interface SegmentOption<T extends string = string> {
  value: T;
  label: string;
  icon?: React.ReactNode;
}

interface SegmentedControlProps<T extends string = string> {
  options: SegmentOption<T>[];
  value: T;
  onChange: (value: T) => void;
  className?: string;
  size?: "sm" | "md";
}

export function SegmentedControl<T extends string = string>({
  options,
  value,
  onChange,
  className = "",
  size = "md",
}: SegmentedControlProps<T>) {
  const paddingClass = size === "sm" ? "px-2.5 py-1 text-xs" : "px-3.5 py-1.5 text-xs";

  return (
    <div
      role="radiogroup"
      className={`inline-flex items-center rounded-lg bg-[#141518] p-0.5 border border-white/[0.08] dark:bg-[#18191c] ${className}`}
    >
      {options.map((option) => {
        const isSelected = option.value === value;
        return (
          <button
            key={option.value}
            type="button"
            role="radio"
            aria-checked={isSelected}
            onClick={() => onChange(option.value)}
            className={`
              flex items-center gap-1.5 rounded-md font-medium tracking-wide transition-all duration-150 press-attenuation
              ${paddingClass}
              ${
                isSelected
                  ? "bg-[#282a2e] text-white shadow-sm font-semibold border border-white/[0.08]"
                  : "text-white/60 hover:text-white/90 hover:bg-white/[0.04]"
              }
            `}
          >
            {option.icon}
            <span>{option.label}</span>
          </button>
        );
      })}
    </div>
  );
}

export default SegmentedControl;
