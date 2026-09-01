import React from "react";

export interface LoadingSpinnerProps {
  size?: number;
  className?: string;
}

/**
 * Global reusable LoadingSpinner component.
 * Renders an animated SVG spinner with emerald styling.
 */
export function LoadingSpinner({
  size = 16,
  className = "",
}: LoadingSpinnerProps) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      xmlns="http://www.w3.org/2000/svg"
      className={`animate-spin ${className}`}
      data-testid="loading-spinner"
      role="status"
      aria-label="Loading"
    >
      <circle
        cx="12"
        cy="12"
        r="10"
        stroke="#10b981"
        strokeWidth="3"
        strokeDasharray="31.4"
        strokeDashoffset="10"
        strokeLinecap="round"
      />
    </svg>
  );
}

export default LoadingSpinner;
