import { Check, Download, MessageSquare, Send, X } from "lucide-react";
import { AnimatePresence, motion } from "motion/react";
import React, { useState } from "react";

export interface FeedbackModalProps {
  isOpen: boolean;
  onClose: () => void;
}

export function FeedbackModal({ isOpen, onClose }: FeedbackModalProps) {
  const [feedbackText, setFeedbackText] = useState("");
  const [category, setCategory] = useState<"bug" | "feature" | "general">("general");
  const [submitted, setSubmitted] = useState(false);

  if (!isOpen) return null;

  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    if (!feedbackText.trim()) return;

    // Persist locally in feedback history
    const existing = JSON.parse(localStorage.getItem("xavier_feedbacks") || "[]");
    existing.push({
      id: Date.now(),
      category,
      text: feedbackText.trim(),
      timestamp: new Date().toISOString(),
    });
    localStorage.setItem("xavier_feedbacks", JSON.stringify(existing));

    setSubmitted(true);
    setTimeout(() => {
      setSubmitted(false);
      setFeedbackText("");
      onClose();
    }, 1200);
  };

  const handleExportDiagnostics = () => {
    const diagnostics = {
      timestamp: new Date().toISOString(),
      userAgent: typeof navigator !== "undefined" ? navigator.userAgent : "N/A",
      activeWorkspace: localStorage.getItem("xavier_active_workspace") || "default",
      theme: localStorage.getItem("vite-ui-theme") || "studio-dark",
      settings: localStorage.getItem("vite-ui-theme-settings"),
      chatPreferences: localStorage.getItem("xavier_chat_preferences"),
      workspaces: localStorage.getItem("xavier_workspaces"),
    };

    const blob = new Blob([JSON.stringify(diagnostics, null, 2)], {
      type: "application/json",
    });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = `xavier-diagnostics-${Date.now()}.json`;
    a.click();
    URL.revokeObjectURL(url);
  };

  return (
    <AnimatePresence>
      <div className="fixed inset-0 z-50 flex items-center justify-center p-4 bg-black/70 backdrop-blur-sm">
        <motion.div
          initial={{ opacity: 0, scale: 0.95, y: 10 }}
          animate={{ opacity: 1, scale: 1, y: 0 }}
          exit={{ opacity: 0, scale: 0.95, y: 10 }}
          transition={{ duration: 0.2 }}
          className="relative w-full max-w-lg bg-[#141518] border border-white/10 rounded-2xl shadow-2xl p-6 text-foreground overflow-hidden"
        >
          {/* Header */}
          <div className="flex items-center justify-between pb-4 border-b border-white/[0.08]">
            <div className="flex items-center gap-2.5">
              <div className="p-2 rounded-lg bg-blue-500/10 border border-blue-500/20 text-blue-400">
                <MessageSquare className="w-4 h-4" />
              </div>
              <div>
                <h3 className="text-sm font-semibold text-white/95">Provide Feedback</h3>
                <p className="text-xs text-white/45">Help us improve the Antigravity experience</p>
              </div>
            </div>
            <button
              type="button"
              onClick={onClose}
              className="p-1 rounded-lg text-white/40 hover:text-white hover:bg-white/10 transition-colors"
              aria-label="Close Feedback Dialog"
            >
              <X className="w-4 h-4" />
            </button>
          </div>

          {/* Form Content */}
          {submitted ? (
            <div className="py-12 flex flex-col items-center justify-center space-y-2 text-center">
              <div className="w-10 h-10 rounded-full bg-emerald-500/20 text-emerald-400 flex items-center justify-center">
                <Check className="w-5 h-5" />
              </div>
              <h4 className="text-sm font-semibold text-white">Thank you for your feedback!</h4>
              <p className="text-xs text-white/50">Your notes have been captured in telemetry.</p>
            </div>
          ) : (
            <form onSubmit={handleSubmit} className="py-4 space-y-4">
              {/* Category selector */}
              <div className="flex gap-2">
                {(["general", "bug", "feature"] as const).map((cat) => (
                  <button
                    key={cat}
                    type="button"
                    onClick={() => setCategory(cat)}
                    className={`px-3 py-1 rounded-lg text-xs font-medium capitalize border transition-all ${
                      category === cat
                        ? "bg-blue-600/20 border-blue-500/40 text-blue-300"
                        : "bg-white/[0.03] border-white/10 text-white/60 hover:text-white"
                    }`}
                  >
                    {cat === "bug" ? "Bug Report" : cat === "feature" ? "Feature Request" : "General"}
                  </button>
                ))}
              </div>

              {/* Text Area */}
              <div>
                <textarea
                  aria-label="Feedback comments"
                  value={feedbackText}
                  onChange={(e) => setFeedbackText(e.target.value)}
                  placeholder="Share details, observed anomalies, or ideas..."
                  rows={4}
                  className="w-full bg-[#181a1f] border border-white/10 rounded-xl p-3 text-xs text-white/90 placeholder-white/30 focus:outline-none focus:border-blue-500/50 resize-none"
                />
              </div>

              {/* Actions & Diagnostics */}
              <div className="flex items-center justify-between pt-2">
                <button
                  type="button"
                  onClick={handleExportDiagnostics}
                  className="inline-flex items-center gap-1.5 text-xs text-white/45 hover:text-white/80 transition-colors"
                >
                  <Download className="w-3.5 h-3.5" />
                  Export Diagnostics JSON
                </button>

                <div className="flex items-center gap-2">
                  <button
                    type="button"
                    onClick={onClose}
                    className="px-3 py-1.5 rounded-lg text-xs text-white/60 hover:text-white hover:bg-white/5 transition-colors"
                  >
                    Cancel
                  </button>
                  <button
                    type="submit"
                    disabled={!feedbackText.trim()}
                    className="inline-flex items-center gap-1.5 px-3.5 py-1.5 rounded-lg bg-blue-600 hover:bg-blue-500 disabled:opacity-40 disabled:cursor-not-allowed text-xs font-medium text-white transition-colors"
                  >
                    <Send className="w-3.5 h-3.5" />
                    Submit
                  </button>
                </div>
              </div>
            </form>
          )}
        </motion.div>
      </div>
    </AnimatePresence>
  );
}
export default FeedbackModal;
