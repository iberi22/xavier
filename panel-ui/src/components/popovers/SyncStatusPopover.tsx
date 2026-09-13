import { RefreshCw, Wifi, X } from "lucide-react";
import { AnimatePresence, motion } from "motion/react";

export interface SyncStatusPopoverProps {
  isOpen: boolean;
  onClose: () => void;
  isSyncing: boolean;
  syncLatencyMs: number | null;
  lastSyncTime: Date | null;
  onTriggerSync: () => Promise<void>;
}

export function SyncStatusPopover({
  isOpen,
  onClose,
  isSyncing,
  syncLatencyMs,
  lastSyncTime,
  onTriggerSync,
}: SyncStatusPopoverProps) {
  return (
    <AnimatePresence>
      {isOpen && (
        <motion.div
          initial={{ opacity: 0, y: 10, scale: 0.95 }}
          animate={{ opacity: 1, y: 0, scale: 1 }}
          exit={{ opacity: 0, y: 10, scale: 0.95 }}
          className="absolute right-0 top-full mt-2 w-60 bg-[#0a0a0a]/95 backdrop-blur-xl border border-cyan-500/30 rounded-xl p-3.5 shadow-2xl z-[80] space-y-3 font-mono"
        >
          <div className="flex items-center justify-between border-b border-white/10 pb-2">
            <div className="flex items-center gap-1.5">
              <Wifi className="w-3.5 h-3.5 text-cyan-400" />
              <span className="text-xs font-bold text-white uppercase tracking-wider">
                Memory Sync
              </span>
            </div>
            <button
              type="button"
              onClick={onClose}
              className="text-white/40 hover:text-white transition-colors cursor-pointer"
              aria-label="Close popover"
            >
              <X className="w-3.5 h-3.5" />
            </button>
          </div>

          <div className="space-y-1.5 text-[10px] text-white/70">
            <div className="flex justify-between">
              <span>Sync Health:</span>
              <span className="text-cyan-400 font-bold">Optimal (98%)</span>
            </div>
            <div className="flex justify-between">
              <span>Peer Connection:</span>
              <span className="text-emerald-400 font-bold">4 Active</span>
            </div>
            {syncLatencyMs !== null && (
              <div className="flex justify-between">
                <span>Last Sync Latency:</span>
                <span className="text-cyan-300 font-bold">{syncLatencyMs}ms</span>
              </div>
            )}
            {lastSyncTime && (
              <div className="flex justify-between">
                <span>Last Synced At:</span>
                <span className="text-white/50">
                  {lastSyncTime.toLocaleTimeString(undefined, {
                    hour: "2-digit",
                    minute: "2-digit",
                    second: "2-digit",
                  })}
                </span>
              </div>
            )}
          </div>

          <div className="pt-1">
            <button
              type="button"
              onClick={onTriggerSync}
              disabled={isSyncing}
              className="w-full py-1.5 px-3 bg-cyan-500/20 hover:bg-cyan-500/30 border border-cyan-500/40 text-cyan-300 rounded-lg text-xs font-bold transition-all flex items-center justify-center gap-2 disabled:opacity-50 cursor-pointer"
              aria-label="Sync Now"
            >
              {isSyncing ? (
                <>
                  <RefreshCw className="w-3.5 h-3.5 animate-spin text-cyan-300" />
                  <span>Syncing...</span>
                </>
              ) : (
                <>
                  <RefreshCw className="w-3.5 h-3.5 text-cyan-300" />
                  <span>Sync Now</span>
                </>
              )}
            </button>
          </div>
        </motion.div>
      )}
    </AnimatePresence>
  );
}

export default SyncStatusPopover;
