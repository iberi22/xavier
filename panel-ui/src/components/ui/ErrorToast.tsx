import { AlertTriangle, X } from "lucide-react";
import { AnimatePresence, motion } from "motion/react";
import React, { useEffect, useState } from "react";

export interface StructuredToastItem {
  id?: string;
  message: string;
  type?: "error" | "rate-limit";
  cooldownSeconds?: number;
  remaining?: number | null;
}

export interface ToastItem {
  id: string;
  message: string;
  type?: "error" | "rate-limit";
  cooldownSeconds?: number;
  remaining?: number | null;
}

export interface ErrorToastProps {
  /**
   * Single message string or null/undefined.
   * When updated, pushes a new message into the toast queue.
   */
  message?: string | null;
  /**
   * Optional queue array of messages or structured toast objects to display directly.
   */
  queue?: Array<string | StructuredToastItem>;
  /**
   * Structured toast objects array.
   */
  structuredToasts?: StructuredToastItem[];
  /**
   * Callback fired when a message or all messages are dismissed.
   */
  onClose?: () => void;
  /**
   * Auto-dismiss delay in milliseconds (defaults to 4000ms).
   */
  autoDismissMs?: number;
}

/**
 * Global ErrorToast component displaying dismissible toast notifications
 * fixed at bottom-left with queue state and auto-dismiss after 4 seconds.
 */
export function ErrorToast({
  message,
  queue: externalQueue,
  structuredToasts,
  onClose,
  autoDismissMs = 4000,
}: ErrorToastProps) {
  const [items, setItems] = useState<ToastItem[]>([]);

  // Listen for window custom events dispatched from ApiClient or global handlers
  useEffect(() => {
    const handleRateLimitEvent = (event: Event) => {
      const customEvent = event as CustomEvent<{
        message?: string;
        retryAfterSeconds?: number;
        remaining?: number | null;
      }>;
      const detail = customEvent.detail;
      if (detail) {
        const id =
          Date.now().toString() + Math.random().toString(36).substring(2, 5);
        const cooldown = detail.retryAfterSeconds;
        const msg =
          detail.message ||
          (cooldown
            ? `Rate limit active. Retry in ${cooldown}s`
            : "Rate limit exceeded");

        setItems((prev) => [
          ...prev,
          {
            id,
            message: msg,
            type: "rate-limit",
            cooldownSeconds: cooldown,
            remaining: detail.remaining,
          },
        ]);
      }
    };

    const handleErrorEvent = (event: Event) => {
      const customEvent = event as CustomEvent<{ message?: string }>;
      if (customEvent.detail?.message) {
        const id =
          Date.now().toString() + Math.random().toString(36).substring(2, 5);
        setItems((prev) => [
          ...prev,
          {
            id,
            message: customEvent.detail.message!,
            type: "error",
          },
        ]);
      }
    };

    window.addEventListener("xavier-rate-limit", handleRateLimitEvent);
    window.addEventListener("xavier-error-toast", handleErrorEvent);

    return () => {
      window.removeEventListener("xavier-rate-limit", handleRateLimitEvent);
      window.removeEventListener("xavier-error-toast", handleErrorEvent);
    };
  }, []);

  // Push new single message into queue when message prop changes
  useEffect(() => {
    if (message) {
      const id = Date.now().toString() + Math.random().toString(36).substring(2, 5);
      setItems((prev) => [...prev, { id, message, type: "error" }]);
    }
  }, [message]);

  // Sync with external queue prop if provided
  useEffect(() => {
    if (externalQueue && externalQueue.length > 0) {
      const newItems = externalQueue.map((item, idx) => {
        const id = `${Date.now()}_${idx}_${Math.random().toString(36).substring(2, 5)}`;
        if (typeof item === "string") {
          return { id, message: item, type: "error" as const };
        }
        return {
          id: item.id || id,
          message: item.message,
          type: item.type || "error",
          cooldownSeconds: item.cooldownSeconds,
          remaining: item.remaining,
        };
      });
      setItems(newItems);
    }
  }, [externalQueue]);

  // Sync with structuredToasts prop if provided
  useEffect(() => {
    if (structuredToasts && structuredToasts.length > 0) {
      const newItems = structuredToasts.map((item, idx) => ({
        id: item.id || `${Date.now()}_st_${idx}_${Math.random().toString(36).substring(2, 5)}`,
        message: item.message,
        type: item.type || "error",
        cooldownSeconds: item.cooldownSeconds,
        remaining: item.remaining,
      }));
      setItems(newItems);
    }
  }, [structuredToasts]);

  // Auto-dismiss the top toast item after autoDismissMs
  useEffect(() => {
    if (items.length === 0) return;

    const timer = setTimeout(() => {
      setItems((prev) => {
        const next = prev.slice(1);
        if (next.length === 0 && onClose) {
          onClose();
        }
        return next;
      });
    }, autoDismissMs);

    return () => clearTimeout(timer);
  }, [items, autoDismissMs, onClose]);

  const dismissItem = (id: string) => {
    setItems((prev) => {
      const next = prev.filter((item) => item.id !== id);
      if (next.length === 0 && onClose) {
        onClose();
      }
      return next;
    });
  };

  if (items.length === 0) {
    return null;
  }

  return (
    <div
      aria-live="assertive"
      className="fixed bottom-4 left-4 z-[100] flex flex-col gap-2 max-w-sm pointer-events-auto"
      data-testid="error-toast-container"
    >
      <AnimatePresence>
        {items.map((item) => {
          const isRateLimit =
            item.type === "rate-limit" || item.cooldownSeconds !== undefined;
          return (
            <motion.div
              key={item.id}
              initial={{ opacity: 0, y: 20, scale: 0.95 }}
              animate={{ opacity: 1, y: 0, scale: 1 }}
              exit={{ opacity: 0, x: -20, scale: 0.95 }}
              transition={{ duration: 0.2 }}
              className={`flex items-center justify-between gap-3 p-3 rounded-xl backdrop-blur-md shadow-xl text-xs font-mono border ${
                isRateLimit
                  ? "bg-[#130d04]/95 border-amber-500/40 text-amber-200"
                  : "bg-[#0a0a0a]/90 border-amber-500/20 text-amber-200"
              }`}
              data-testid="error-toast-item"
            >
              <div className="flex items-center gap-2.5 min-w-0 flex-wrap">
                <span
                  className="text-amber-400 shrink-0 select-none"
                  aria-hidden="true"
                >
                  {isRateLimit ? "⏱️" : "⚠️"}
                </span>
                <span className="truncate">{item.message}</span>
                {item.cooldownSeconds !== undefined &&
                  item.cooldownSeconds !== null && (
                    <span
                      data-testid="rate-limit-cooldown-badge"
                      className="px-1.5 py-0.5 rounded text-[10px] bg-amber-500/20 text-amber-300 font-semibold border border-amber-500/30"
                    >
                      Cooldown: {item.cooldownSeconds}s
                    </span>
                  )}
                {item.remaining !== undefined && item.remaining !== null && (
                  <span
                    data-testid="rate-limit-remaining-badge"
                    className="text-[10px] text-amber-400/80"
                  >
                    Remaining: {item.remaining}
                  </span>
                )}
              </div>
              <button
                type="button"
                onClick={() => dismissItem(item.id)}
                className="text-amber-400/50 hover:text-amber-200 transition-colors p-1 rounded-md shrink-0"
                aria-label="Dismiss error"
              >
                <X className="w-3.5 h-3.5" />
              </button>
            </motion.div>
          );
        })}
      </AnimatePresence>
    </div>
  );
}

export default ErrorToast;
