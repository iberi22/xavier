import { Loader2 } from "lucide-react";

interface LoadingSpinnerProps {
  size?: number;
  className?: string;
}

export function LoadingSpinner({
  size = 16,
  className = "",
}: LoadingSpinnerProps) {
  return (
    <Loader2
      size={size}
      className={`animate-spin text-emerald-400 ${className}`}
      aria-label="Loading..."
    />
  );
}

export default LoadingSpinner;
