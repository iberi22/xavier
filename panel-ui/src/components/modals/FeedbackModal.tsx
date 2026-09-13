import { CheckCircle2, Download, MessageSquare, Send, X } from "lucide-react";
import { AnimatePresence, motion } from "motion/react";
import React, { useEffect, useState } from "react";

interface FeedbackModalProps {
  isOpen: boolean;
  onClose: () => void;
  onSubmitFeedback?: (feedback: { category: string; message: string }) => void;
  onExportDiagnostics?: () => void;
}

export const FeedbackModal: React.FC<FeedbackModalProps> = ({
  isOpen,
  onClose,
  onSubmitFeedback,
  onExportDiagnostics,
}) => {
  const [category, setCategory] = useState("general");
  const [message, setMessage] = useState("");
  const [isSubmitted, setIsSubmitted] = useState(false);
  const [isExporting, setIsExporting] = useState(false);

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

  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    if (!message.trim()) return;

    if (onSubmitFeedback) {
      onSubmitFeedback({ category, message });
    }
    setIsSubmitted(true);
    setTimeout(() => {
      setIsSubmitted(false);
      setMessage("");
      onClose();
    }, 1800);
  };

  const handleExport = () => {
    setIsExporting(true);
    if (onExportDiagnostics) {
      onExportDiagnostics();
    } else {
      // Default diagnostics JSON generator and trigger download
      const diagnosticsData = {
        timestamp: new Date().toISOString(),
        userAgent: typeof navigator !== "undefined" ? navigator.userAgent : "unknown",
        platform: typeof navigator !== "undefined" ? navigator.platform : "unknown",
        theme: typeof document !== "undefined" ? document.documentElement.getAttribute("data-theme") || "studio-dark" : "unknown",
        url: typeof window !== "undefined" ? window.location.href : "unknown",
        appVersion: "0.2.1",
        status: "healthy",
      };

      const blob = new Blob([JSON.stringify(diagnosticsData, null, 2)], {
        type: "application/json",
      });
      const url = URL.createObjectURL(blob);
      const a = document.createElement("a");
      a.href = url;
      a.download = `xavier-diagnostics-${Date.now()}.json`;
      a.click();
      URL.revokeObjectURL(url);
    }
    setTimeout(() => setIsExporting(false), 1000);
  };

  return (
    <AnimatePresence>
      <div
        className="fixed inset-0 z-50 flex items-center justify-center p-4 bg-black/60 backdrop-blur-md"
        role="dialog"
        aria-modal="true"
        aria-labelledby="feedback-modal-title"
      >
        {/* Backdrop click dismiss */}
        <div
          className="absolute inset-0"
          onClick={onClose}
          aria-hidden="true"
        />

        <motion.div
          initial={{ opacity: 0, scale: 0.95, y: 10 }}
          animate={{ opacity: 1, scale: 1, y: 0 }}
          exit={{ opacity: 0, scale: 0.95, y: 10 }}
          className="relative z-10 w-full max-w-lg overflow-hidden rounded-2xl bg-[#0f1013] border border-white/10 shadow-2xl p-6 text-white"
        >
          {/* Header */}
          <div className="flex items-center justify-between pb-4 mb-4 border-b border-white/10">
            <div className="flex items-center gap-3">
              <div className="p-2 rounded-xl bg-[#39ff14]/10 text-[#39ff14] border border-[#39ff14]/20">
                <MessageSquare className="w-5 h-5" />
              </div>
              <div>
                <h2
                  id="feedback-modal-title"
                  className="text-lg font-semibold tracking-tight text-white"
                >
                  Provide Feedback
                </h2>
                <p className="text-xs text-white/50">
                  Help us improve Xavier or export diagnostic context
                </p>
              </div>
            </div>
            <button
              type="button"
              onClick={onClose}
              className="p-2 rounded-lg text-white/40 hover:text-white hover:bg-white/10 transition-colors"
              aria-label="Close feedback modal"
            >
              <X className="w-5 h-5" />
            </button>
          </div>

          {isSubmitted ? (
            <motion.div
              initial={{ opacity: 0, y: 10 }}
              animate={{ opacity: 1, y: 0 }}
              className="py-10 flex flex-col items-center justify-center text-center space-y-3"
            >
              <CheckCircle2 className="w-12 h-12 text-[#39ff14] animate-bounce" />
              <h3 className="text-lg font-medium text-white">Feedback Submitted</h3>
              <p className="text-xs text-white/60 max-w-xs">
                Thank you for your response! Your insights help optimize the system.
              </p>
            </motion.div>
          ) : (
            <form onSubmit={handleSubmit} className="space-y-4">
              <div>
                <label className="block text-xs font-medium uppercase tracking-wider text-white/60 mb-2">
                  Category
                </label>
                <select
                  value={category}
                  onChange={(e) => setCategory(e.target.value)}
                  className="w-full bg-[#050505]/80 border border-white/10 focus:border-[#39ff14] text-white/90 text-sm px-4 py-2.5 rounded-xl outline-none transition-all cursor-pointer"
                >
                  <option value="general" className="bg-[#0f1013]">General Feedback</option>
                  <option value="bug" className="bg-[#0f1013]">Bug Report</option>
                  <option value="feature" className="bg-[#0f1013]">Feature Request</option>
                  <option value="performance" className="bg-[#0f1013]">Performance & UX</option>
                </select>
              </div>

              <div>
                <label className="block text-xs font-medium uppercase tracking-wider text-white/60 mb-2">
                  Your Message
                </label>
                <textarea
                  rows={4}
                  value={message}
                  onChange={(e) => setMessage(e.target.value)}
                  placeholder="Describe your feedback or issue..."
                  required
                  className="w-full bg-[#050505]/80 border border-white/10 focus:border-[#39ff14] text-white/90 text-sm p-4 rounded-xl outline-none transition-all resize-none placeholder:text-white/30"
                />
              </div>

              <div className="pt-2 flex items-center justify-between gap-3 border-t border-white/5">
                <button
                  type="button"
                  onClick={handleExport}
                  disabled={isExporting}
                  className="flex items-center gap-2 px-3.5 py-2.5 rounded-xl bg-white/5 hover:bg-white/10 border border-white/10 text-xs font-medium text-white/80 transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[#39ff14]"
                >
                  <Download className="w-4 h-4 text-[#39ff14]" />
                  {isExporting ? "Exporting..." : "Export Diagnostics JSON"}
                </button>

                <div className="flex items-center gap-2">
                  <button
                    type="button"
                    onClick={onClose}
                    className="px-4 py-2.5 rounded-xl text-xs font-medium text-white/60 hover:text-white hover:bg-white/5 transition-colors"
                  >
                    Cancel
                  </button>
                  <button
                    type="submit"
                    disabled={!message.trim()}
                    className="flex items-center gap-2 px-4 py-2.5 rounded-xl bg-[#39ff14] text-black font-semibold text-xs shadow-[0_0_15px_rgba(57,255,20,0.3)] hover:brightness-110 active:scale-95 transition-all disabled:opacity-50 disabled:cursor-not-allowed"
                  >
                    <Send className="w-3.5 h-3.5" />
                    Submit Feedback
                  </button>
                </div>
              </div>
            </form>
          )}
        </motion.div>
      </div>
    </AnimatePresence>
  );
};

export default FeedbackModal;
