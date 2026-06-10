import { Loader2, Send } from "lucide-react";

export function IntegrationsStep({
  token,
  onChangeToken,
  onComplete,
  isSaving,
}: {
  token: string;
  onChangeToken: (val: string) => void;
  onComplete: () => void;
  isSaving: boolean;
}) {
  return (
    <div className="space-y-6 animate-in fade-in duration-500">
      <div className="text-center space-y-2">
        <h2 className="text-2xl font-bold text-white">EXTERNAL_UPLINK</h2>
        <p className="text-emerald-500/70 text-sm">
          Configure Telegram Bot Integration
        </p>
      </div>

      <div className="space-y-4">
        <div className="bg-neutral-950 p-4 border border-emerald-900/30 rounded">
          <label className="block text-sm font-semibold text-emerald-400 mb-2 flex items-center gap-2">
            <Send className="w-4 h-4" /> Telegram Bot Token
          </label>
          <input
            type="password"
            value={token}
            onChange={(e) => onChangeToken(e.target.value)}
            placeholder="123456789:ABCdefGHIjklMNOpqrsTUVwxyz"
            className="w-full bg-black border border-emerald-900 rounded p-2 text-emerald-300 focus:outline-none focus:border-emerald-500 transition-colors font-mono placeholder:text-neutral-700"
          />
          <p className="text-xs text-neutral-500 mt-2">
            Optional. Leave blank to skip. Required to receive notifications and
            interact via Telegram.
          </p>
        </div>
      </div>

      <div className="flex justify-between items-center pt-4">
        <span className="text-xs text-neutral-500 uppercase tracking-widest">
          Initialization Ready
        </span>
        <button
          onClick={onComplete}
          disabled={isSaving}
          className="px-6 py-2 bg-emerald-600 hover:bg-emerald-500 disabled:opacity-50 disabled:cursor-not-allowed text-black font-bold rounded transition-colors flex items-center gap-2"
        >
          {isSaving ? (
            <>
              <Loader2 className="w-4 h-4 animate-spin" /> WRITING_CONFIG...
            </>
          ) : (
            "INITIALIZE_SYSTEM"
          )}
        </button>
      </div>
    </div>
  );
}
