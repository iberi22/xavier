import { Command, X } from "lucide-react";
import { AnimatePresence, motion } from "motion/react";
import React, { useEffect } from "react";

export interface ShortcutsModalProps {
  isOpen: boolean;
  onClose: () => void;
}

interface ShortcutItem {
  keys: string[];
  description: string;
  category: "Navigation" | "Theme & Display" | "Actions";
}

const SHORTCUTS: ShortcutItem[] = [
  {
    keys: ["Ctrl", "K"],
    description: "Focus prompt and command palette",
    category: "Navigation",
  },
  {
    keys: ["Ctrl", ","],
    description: "Open / Close Control Node Settings",
    category: "Navigation",
  },
  {
    keys: ["Ctrl", "Shift", "L"],
    description: "Toggle Light / Dark mode",
    category: "Theme & Display",
  },
  {
    keys: ["Escape"],
    description: "Close active modal or dismiss popover",
    category: "Actions",
  },
  {
    keys: ["Ctrl", "Shift", "P"],
    description: "Switch active project workspace",
    category: "Navigation",
  },
  {
    keys: ["Ctrl", "Enter"],
    description: "Send prompt message to agent",
    category: "Actions",
  },
];

export function ShortcutsModal({ isOpen, onClose }: ShortcutsModalProps) {
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape" && isOpen) {
        onClose();
      }
    };
    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [isOpen, onClose]);

  if (!isOpen) return null;

  const categories = ["Navigation", "Theme & Display", "Actions"] as const;

  return (
    <AnimatePresence>
      <div className="fixed inset-0 z-50 flex items-center justify-center p-4 bg-black/20 dark:bg-black/70 backdrop-blur-sm">
        <motion.div
          initial={{ opacity: 0, scale: 0.95, y: 10 }}
          animate={{ opacity: 1, scale: 1, y: 0 }}
          exit={{ opacity: 0, scale: 0.95, y: 10 }}
          transition={{ duration: 0.2 }}
          className="relative w-full max-w-lg bg-surface-card dark:bg-[#141518] border border-black/10 dark:border-white/10 rounded-2xl shadow-2xl p-6 text-foreground overflow-hidden"
        >
          {/* Header */}
          <div className="flex items-center justify-between pb-4 border-b border-black/[0.08] dark:border-white/[0.08]">
            <div className="flex items-center gap-2.5">
              <div className="p-2 rounded-lg bg-black/5 dark:bg-white/5 border border-black/10 dark:border-white/10 text-black/80 dark:text-white/80">
                <Command className="w-4 h-4" />
              </div>
              <div>
                <h3 className="text-sm font-semibold text-black/95 dark:text-white/95">Keyboard Shortcuts</h3>
                <p className="text-xs text-black/45 dark:text-white/45">Quick navigation & action key bindings</p>
              </div>
            </div>
            <button
              type="button"
              onClick={onClose}
              className="p-1 rounded-lg text-black/40 dark:text-white/40 hover:text-black dark:hover:text-white hover:bg-black/10 dark:hover:bg-white/10 transition-colors"
              aria-label="Close Shortcuts Dialog"
            >
              <X className="w-4 h-4" />
            </button>
          </div>

          {/* Shortcuts List */}
          <div className="py-4 space-y-5 max-h-[60vh] overflow-y-auto pr-1">
            {categories.map((cat) => {
              const items = SHORTCUTS.filter((s) => s.category === cat);
              if (items.length === 0) return null;
              return (
                <div key={cat} className="space-y-2">
                  <div className="text-[11px] font-semibold uppercase tracking-wider text-black/40 dark:text-white/40 px-1">
                    {cat}
                  </div>
                  <div className="rounded-xl border border-black/[0.06] dark:border-white/[0.06] bg-black/[0.02] dark:bg-white/[0.02] divide-y divide-black/[0.04] dark:divide-white/[0.04]">
                    {items.map((item) => (
                      <div
                        key={item.description}
                        className="flex items-center justify-between px-3 py-2 text-xs"
                      >
                        <span className="text-black/80 dark:text-white/80 font-medium">{item.description}</span>
                        <div className="flex items-center gap-1">
                          {item.keys.map((k) => (
                            <kbd
                              key={k}
                              className="px-2 py-0.5 rounded-md bg-black/5 dark:bg-[#1e2025] border border-black/15 dark:border-white/15 text-[11px] font-mono text-black/90 dark:text-white/90 shadow-sm"
                            >
                              {k}
                            </kbd>
                          ))}
                        </div>
                      </div>
                    ))}
                  </div>
                </div>
              );
            })}
          </div>

          {/* Footer */}
          <div className="pt-3 border-t border-black/[0.08] dark:border-white/[0.08] flex justify-end">
            <button
              type="button"
              onClick={onClose}
              className="px-3.5 py-1.5 rounded-lg bg-black/10 dark:bg-white/10 hover:bg-black/15 dark:hover:bg-white/15 text-xs font-medium text-black dark:text-white transition-colors"
            >
              Done
            </button>
          </div>
        </motion.div>
      </div>
    </AnimatePresence>
  );
}
export default ShortcutsModal;
