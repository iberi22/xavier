import React, { useEffect, useId, useRef, useState } from "react";
import {
  cancelMalocaToken,
  isMalocaAuthPending,
  submitMalocaToken,
  subscribeMalocaAuth,
} from "../authPrompt";

/** Asks for the Xavier token after a 401. Rendered once by MalocaView. */
export function MalocaAuthPrompt() {
  const [open, setOpen] = useState(isMalocaAuthPending());
  const [value, setValue] = useState("");
  const inputRef = useRef<HTMLInputElement>(null);
  const titleId = useId();
  const descId = useId();

  useEffect(() => subscribeMalocaAuth(setOpen), []);
  useEffect(() => {
    if (open) inputRef.current?.focus();
    else setValue("");
  }, [open]);

  if (!open) return null;

  const onSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    submitMalocaToken(value);
  };

  return (
    <div className="absolute inset-0 z-50 flex items-center justify-center bg-black/70 p-4">
      <form
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        aria-describedby={descId}
        onSubmit={onSubmit}
        onKeyDown={(e) => {
          if (e.key === "Escape") cancelMalocaToken();
        }}
        className="w-full max-w-md rounded-2xl border border-white/10 bg-[#0a0a0a] p-6 flex flex-col gap-4"
      >
        <h2 id={titleId} className="text-lg font-bold text-white">
          Token required
        </h2>
        <p id={descId} className="text-sm text-white/60">
          This data is private. Enter your Xavier access token. It is stored only on this device and sent only to
          this server.
        </p>
        <label htmlFor={`${titleId}-token`} className="text-xs text-white/60 uppercase tracking-widest">
          Access token
        </label>
        <input
          id={`${titleId}-token`}
          ref={inputRef}
          type="password"
          autoComplete="off"
          value={value}
          onChange={(e) => setValue(e.target.value)}
          className="w-full bg-white/5 border border-white/10 rounded-lg p-3 text-sm focus:border-[#39ff14] focus:outline-none font-mono"
        />
        <div className="flex justify-end gap-2">
          <button
            type="button"
            onClick={cancelMalocaToken}
            className="px-4 py-2 text-xs font-mono rounded-lg text-white/60 hover:text-white focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-emerald-500"
          >
            Cancel
          </button>
          <button
            type="submit"
            disabled={!value.trim()}
            className="px-4 py-2 text-xs font-mono rounded-lg bg-emerald-500/10 text-emerald-400 border border-emerald-500/30 disabled:opacity-40 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-emerald-500"
          >
            Save and retry
          </button>
        </div>
      </form>
    </div>
  );
}
