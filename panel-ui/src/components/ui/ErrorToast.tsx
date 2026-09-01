import { AlertTriangle, X } from "lucide-react";
import { AnimatePresence, motion } from "motion/react";
import React, { useEffect, useState } from "react";

export interface ErrorToastProps {
  /**
   * Single message string or null/undefined.
   * When updated, pushes a new message into the toast queue.
   */
  message?: string | null;
  /**
   * Optional queue array of messages to display directly.
   */
  queue?: string[];
  /**
   * Callback fired when a message or all messages are dismissed.
   */
  onClose?: () => void;
  /**
   * Auto-dismiss delay in milliseconds (defaults to 4000ms).
   */
  autoDismissMs?: number;
}

export interface ToastItem {
  id: string;
  message: string;
}

/**
 * Global ErrorToast component displaying dismissible toast notifications
 * fixed at bottom-left with queue state and auto-dismiss after 4 seconds.
 */
export function ErrorToast({
  message,
  queue: externalQueue,
  onClose,
  autoDismissMs = 4000,
}: ErrorToastProps) {
  const [items, setItems] = useState<ToastItem[]>([]);

  // Push new single message into queue when message prop changes
  useEffect(() => {
    if (message) {
      const id = Date.now().toString() + Math.random().toString(36).substring(2, 5);
      setItems((prev) => [...prev, { id, message }]);
    }
  }, [message]);

  // Sync with external queue prop if provided
  useEffect(() => {
    if (externalQueue && externalQueue.length > 0) {
      const newItems = externalQueue.map((msg, idx) => ({
        id: `${Date.now()}_${idx}_${Math.random().toString(36).substring(2, 5)}`,
        message: msg,
      }));
      setItems(newItems);
    }
  }, [externalQueue]);

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
        {items.map((item) => (
          <motion.div
            key={item.id}
            initial={{ opacity: 0, y: 20, scale: 0.95 }}
            animate={{ opacity: 1, y: 0, scale: 1 }}
            exit={{ opacity: 0, x: -20, scale: 0.95 }}
            transition={{ duration: 0.2 }}
            className="flex items-center justify-between gap-3 p-3 rounded-xl bg-[#0a0a0a]/90 backdrop-blur-md border border-amber-500/20 shadow-xl text-amber-200 text-xs font-mono"
            data-testid="error-toast-item"
          >
            <div className="flex items-center gap-2.5 min-w-0">
              <span className="text-amber-400 shrink-0 select-none" aria-hidden="true">
                ⚠️
              </span>
              <span className="truncate">{item.message}</span>
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
        ))}
      </AnimatePresence>
    </div>
  );
}

export default ErrorToast;
