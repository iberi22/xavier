import { AlertTriangle, X } from "lucide-react";
import { AnimatePresence, motion } from "motion/react";

export interface SystemAlert {
  id: string;
  level: string;
  message: string;
  component: string;
  created_at: string;
}

interface SystemAlertBannerProps {
  alerts: SystemAlert[];
  onDismiss: (id: string) => void;
  onOpenConfig: () => void;
}

export default function SystemAlertBanner({
  alerts,
  onDismiss,
  onOpenConfig,
}: SystemAlertBannerProps) {
  if (alerts.length === 0) return null;

  return (
    <div className="absolute top-16 left-0 right-0 z-40 flex flex-col items-center pointer-events-none p-4 space-y-2">
      <AnimatePresence>
        {alerts.map((alert) => {
          const isEmbeddingError = alert.message
            .toLowerCase()
            .includes("embedding backend unavailable");

          return (
            <motion.div
              key={alert.id}
              role="alert"
              aria-live="assertive"
              initial={{ opacity: 0, y: -20, scale: 0.95 }}
              animate={{ opacity: 1, y: 0, scale: 1 }}
              exit={{ opacity: 0, scale: 0.95, transition: { duration: 0.2 } }}
              className="bg-red-950/80 border border-red-500/50 backdrop-blur-md rounded-lg p-4 shadow-lg flex items-start max-w-2xl w-full pointer-events-auto"
            >
              <div className="flex-shrink-0 mt-0.5">
                <AlertTriangle className="w-5 h-5 text-red-500" aria-hidden="true" />
              </div>
              <div className="ml-3 flex-1">
                <h3 className="text-sm font-medium text-red-400 font-mono">
                  SYSTEM ALERT: {alert.component.toUpperCase()}
                </h3>
                <p className="mt-1 text-sm text-red-200/80 leading-relaxed">
                  {alert.message}
                </p>

                {isEmbeddingError && (
                  <div className="mt-3 bg-black/40 rounded p-3 text-xs text-red-100">
                    <p className="mb-2">
                      Fallo en embeddings locales. Configure una API externa en
                      Ajustes, o recompile Xavier activando CUDA/GPU para
                      soportar el modelo local.
                    </p>
                    <button
                      type="button"
                      onClick={onOpenConfig}
                      className="px-3 py-1.5 bg-red-500/20 hover:bg-red-500/40 border border-red-500/30 rounded transition-colors text-red-100 font-medium focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-red-500/50"
                    >
                      Abrir Ajustes de API
                    </button>
                  </div>
                )}
              </div>
              <button
                type="button"
                aria-label="Dismiss alert"
                title="Dismiss alert"
                onClick={() => onDismiss(alert.id)}
                className="ml-4 flex-shrink-0 text-red-400 hover:text-white transition-colors rounded focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-red-500/50"
              >
                <X className="w-5 h-5" aria-hidden="true" />
              </button>
            </motion.div>
          );
        })}
      </AnimatePresence>
    </div>
  );
}
