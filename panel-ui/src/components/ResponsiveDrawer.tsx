import { X } from "lucide-react";
import { AnimatePresence, motion } from "motion/react";
import type React from "react";
import { useEffect } from "react";

export interface ResponsiveDrawerProps {
  isOpen: boolean;
  onClose: () => void;
  title?: React.ReactNode;
  children: React.ReactNode;
  className?: string;
}

export function ResponsiveDrawer({
  isOpen,
  onClose,
  title,
  children,
  className = "",
}: ResponsiveDrawerProps) {
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape" && isOpen) {
        onClose();
      }
    };
    if (isOpen) {
      window.addEventListener("keydown", handleKeyDown);
    }
    return () => {
      window.removeEventListener("keydown", handleKeyDown);
    };
  }, [isOpen, onClose]);

  return (
    <AnimatePresence>
      {isOpen && (
        <div className="fixed inset-0 z-50 flex items-end sm:items-center justify-center">
          {/* Backdrop */}
          <motion.div
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
            exit={{ opacity: 0 }}
            onClick={onClose}
            className="fixed inset-0 bg-black/60 backdrop-blur-sm"
            aria-hidden="true"
            data-testid="drawer-backdrop"
          />

          {/* Drawer / Bottom Sheet Container */}
          <motion.div
            role="dialog"
            aria-modal="true"
            aria-label={typeof title === "string" ? title : "Drawer"}
            initial={{ y: "100%", opacity: 0 }}
            animate={{ y: 0, opacity: 1 }}
            exit={{ y: "100%", opacity: 0 }}
            transition={{ type: "spring", damping: 25, stiffness: 200 }}
            className={`relative z-10 w-full sm:max-w-lg bg-[#0d0e10] border-t sm:border border-white/10 rounded-t-[28px] sm:rounded-[28px] p-6 shadow-2xl max-h-[85vh] sm:max-h-[90vh] flex flex-col overflow-hidden pb-[env(safe-area-inset-bottom)] ${className}`}
          >
            {/* Top Grab Handle for Mobile */}
            <div className="w-12 h-1.5 bg-white/20 rounded-full mx-auto mb-4 shrink-0 sm:hidden" />

            {/* Header */}
            <div className="flex items-center justify-between pb-4 border-b border-white/10 shrink-0">
              {title ? (
                typeof title === "string" ? (
                  <h3 className="text-lg font-medium text-white tracking-tight">
                    {title}
                  </h3>
                ) : (
                  title
                )
              ) : (
                <div />
              )}
              <button
                type="button"
                onClick={onClose}
                className="p-2 text-white/50 hover:text-white hover:bg-white/10 rounded-full transition-colors focus:outline-none focus:ring-2 focus:ring-[#39ff14]/50"
                aria-label="Close drawer"
              >
                <X className="w-5 h-5" />
              </button>
            </div>

            {/* Content Area */}
            <div className="flex-1 overflow-y-auto pt-4 text-white/90">
              {children}
            </div>
          </motion.div>
        </div>
      )}
    </AnimatePresence>
  );
}

export default ResponsiveDrawer;
