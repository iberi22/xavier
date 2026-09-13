import { X } from "lucide-react";
import { AnimatePresence, motion } from "motion/react";
import React, { useEffect } from "react";

interface ResponsiveDrawerProps {
  isOpen: boolean;
  onClose: () => void;
  title?: string;
  children: React.ReactNode;
}

export const ResponsiveDrawer: React.FC<ResponsiveDrawerProps> = ({
  isOpen,
  onClose,
  title,
  children,
}) => {
  useEffect(() => {
    if (isOpen) {
      document.body.style.overflow = "hidden";
    } else {
      document.body.style.overflow = "";
    }
    return () => {
      document.body.style.overflow = "";
    };
  }, [isOpen]);

  return (
    <AnimatePresence>
      {isOpen && (
        <div className="fixed inset-0 z-[100] flex flex-col justify-end sm:justify-center items-center pointer-events-auto">
          {/* Backdrop */}
          <motion.div
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
            exit={{ opacity: 0 }}
            onClick={onClose}
            className="absolute inset-0 bg-black/60 backdrop-blur-sm"
          />

          {/* Drawer / Modal Container */}
          <motion.div
            initial={{ y: "100%", opacity: 0 }}
            animate={{ y: 0, opacity: 1 }}
            exit={{ y: "100%", opacity: 0 }}
            transition={{ type: "spring", damping: 28, stiffness: 300 }}
            className="relative z-10 w-full max-w-xl max-h-[85vh] rounded-t-2xl sm:rounded-2xl surface-borderless flex flex-col overflow-hidden bg-[#141518] shadow-2xl pb-[env(safe-area-inset-bottom,16px)]"
          >
            {/* Header */}
            <div className="flex items-center justify-between px-5 py-4 border-b border-white/[0.06]">
              {title ? (
                <h3 className="text-sm font-semibold text-foreground tracking-wide">
                  {title}
                </h3>
              ) : (
                <div className="w-8 h-1 rounded-full bg-white/20 mx-auto" />
              )}
              <button
                type="button"
                onClick={onClose}
                className="p-1.5 rounded-lg border border-white/10 hover:bg-white/5 text-foreground/60 hover:text-foreground press-attenuation"
                aria-label="Cerrar modal"
              >
                <X className="w-4 h-4" />
              </button>
            </div>

            {/* Body */}
            <div className="flex-1 overflow-y-auto p-4 sm:p-6 text-foreground">
              {children}
            </div>
          </motion.div>
        </div>
      )}
    </AnimatePresence>
  );
};

export default ResponsiveDrawer;
