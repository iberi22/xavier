import { BrainCircuit, FolderPlus, Mic, Send } from "lucide-react";
import React, { useState, useCallback, useRef } from "react";

interface InputAreaProps {
  onSendMessage: (text: string) => void;
  onOpenConfig: () => void;
  onSystemMessage?: (text: string) => void;
}

/**
 * ⚡ Bolt Performance Optimization & 🎨 Palette Ola Theme
 *
 * 💡 What: Wrapped InputArea in React.memo() with studio capsule aesthetic & bordered controls.
 * 🎯 Why: Replaced hardcoded neon green focus rings and glow effects with theme-aware bordered controls and smooth press attenuation.
 * 📊 Impact: Prevents unnecessary renders and improves visual consistency and accessibility across themes.
 */
export default React.memo(function InputArea({
  onSendMessage,
  onOpenConfig,
  onSystemMessage,
}: InputAreaProps) {
  const [inputText, setInputText] = useState("");
  const [isRecording, setIsRecording] = useState(false);
  const [isTranscribing, setIsTranscribing] = useState(false);
  const fileInputRef = useRef<HTMLInputElement>(null);

  const handleMicClick = useCallback(() => {
    if (isRecording) {
      setIsRecording(false);
      setIsTranscribing(true);
      // Simulate transcription delay
      setTimeout(() => {
        setIsTranscribing(false);
        setInputText(
          (prev) => prev + (prev ? " " : "") + "Audio transcript processed.",
        );
      }, 2000);
    } else {
      setIsRecording(true);
      setIsTranscribing(false);
    }
  }, [isRecording]);

  const handleFileChange = useCallback(
    (e: React.ChangeEvent<HTMLInputElement>) => {
      const files = e.target.files;
      if (files && files.length > 0) {
        const firstFile = files[0] as File & { webkitRelativePath?: string };
        const folder = firstFile.webkitRelativePath
          ? firstFile.webkitRelativePath.split("/")[0]
          : "directorio";
        onSystemMessage?.(
          `Carpeta seleccionada: ${folder} (${files.length} archivos)`,
        );
      }
      e.target.value = "";
    },
    [onSystemMessage],
  );

  const handleFolderClick = useCallback(async () => {
    const isTauri =
      typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

    if (isTauri) {
      try {
        const { open } = await import("@tauri-apps/plugin-dialog");
        const { invoke } = await import("@tauri-apps/api/core");

        const selected = await open({
          directory: true,
          multiple: false,
        });

        if (selected && typeof selected === "string") {
          onSystemMessage?.(`Iniciando escaneo del proyecto en: ${selected}...`);
          const result = await invoke<string>("scan_project_folder", {
            path: selected,
          });
          onSystemMessage?.(`✅ ${result}`);
        }
      } catch (err) {
        onSystemMessage?.(`❌ Error al escanear: ${err}`);
      }
    } else {
      fileInputRef.current?.click();
    }
  }, [onSystemMessage]);

  const handleSend = useCallback(() => {
    if (!inputText.trim()) return;
    onSendMessage(inputText);
    setInputText("");
  }, [inputText, onSendMessage]);

  return (
    <div className="absolute bottom-6 sm:bottom-8 left-1/2 -translate-x-1/2 w-full max-w-2xl px-3 sm:px-4 pointer-events-auto z-10">
      <input
        type="file"
        ref={fileInputRef}
        // @ts-expect-error webkitdirectory is non-standard but supported by Chrome/Edge
        webkitdirectory=""
        style={{ display: "none" }}
        onChange={handleFileChange}
        aria-hidden="true"
      />
      <div className="bg-[#151619]/90 dark:bg-[#151619]/90 backdrop-blur-xl border border-white/[0.06] rounded-2xl p-2 flex items-center gap-1.5 sm:gap-2 relative overflow-hidden transition-all duration-300 focus-within:border-white/20 shadow-2xl">
        {/* Animated background when recording */}
        {isRecording && (
          <div className="absolute inset-0 bg-white/[0.03] animate-pulse pointer-events-none" />
        )}

        <button
          type="button"
          onClick={onOpenConfig}
          aria-label="Open Control Node"
          className="relative z-10 w-10 h-10 sm:w-11 sm:h-11 flex items-center justify-center rounded-xl border border-white/10 dark:border-white/10 transition-all duration-200 hover:bg-white/[0.06] press-attenuation active:scale-95 text-white/80 hover:text-white group focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-white/20 shrink-0"
          title="Open Control Node"
        >
          <BrainCircuit
            className="w-5 h-5 transition-all duration-200"
            strokeWidth={1.5}
            aria-hidden="true"
          />
        </button>

        <button
          type="button"
          onClick={handleFolderClick}
          aria-label="Add project codebase"
          className="relative z-10 w-10 h-10 sm:w-11 sm:h-11 flex items-center justify-center rounded-xl border border-white/10 dark:border-white/10 transition-all duration-200 hover:bg-white/[0.06] press-attenuation active:scale-95 text-white/60 hover:text-white/90 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-white/20 shrink-0"
          title="Agregar Codebase (Proyecto Git)"
        >
          <FolderPlus className="w-5 h-5" aria-hidden="true" />
        </button>

        <div
          className="w-px h-6 bg-white/10 relative z-10 mx-0.5 sm:mx-1 shrink-0"
          aria-hidden="true"
        />

        <button
          type="button"
          onClick={handleMicClick}
          aria-label={isRecording ? "Stop recording" : "Record audio"}
          aria-pressed={isRecording}
          className={`relative z-10 w-10 h-10 sm:w-11 sm:h-11 flex items-center justify-center rounded-xl border border-white/10 dark:border-white/10 transition-all duration-200 hover:bg-white/[0.06] press-attenuation active:scale-95 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-white/20 shrink-0 ${
            isRecording
              ? "text-rose-400 border-rose-500/30 bg-rose-500/10"
              : "text-white/60 hover:text-white/90"
          }`}
          title={isRecording ? "Stop recording" : "Record audio"}
        >
          {isRecording ? (
            <div
              className="flex gap-[3px] items-center justify-center h-full"
              aria-hidden="true"
            >
              <div
                className="w-[3px] bg-rose-400 rounded-full animate-[audioBar_1s_ease-in-out_infinite_0ms]"
                style={{ height: "12px" }}
              />
              <div
                className="w-[3px] bg-rose-400 rounded-full animate-[audioBar_1s_ease-in-out_infinite_100ms]"
                style={{ height: "24px" }}
              />
              <div
                className="w-[3px] bg-rose-400 rounded-full animate-[audioBar_1s_ease-in-out_infinite_200ms]"
                style={{ height: "16px" }}
              />
              <div
                className="w-[3px] bg-rose-400 rounded-full animate-[audioBar_1s_ease-in-out_infinite_300ms]"
                style={{ height: "20px" }}
              />
            </div>
          ) : (
            <Mic className="w-5 h-5" aria-hidden="true" />
          )}
        </button>

        <div className="flex-1 relative z-10 flex flex-col justify-center min-h-[40px] sm:min-h-[44px] min-w-0">
          {isTranscribing ? (
            <div
              className="flex items-center gap-2 px-2 text-white/70 text-sm italic font-medium w-full animate-pulse"
              role="status"
              aria-live="polite"
              aria-busy="true"
            >
              Transcribing
              <span className="flex gap-1" aria-hidden="true">
                <span
                  className="w-1 h-1 bg-white/70 rounded-full animate-bounce"
                  style={{ animationDelay: "-0.3s" }}
                ></span>
                <span
                  className="w-1 h-1 bg-white/70 rounded-full animate-bounce"
                  style={{ animationDelay: "-0.15s" }}
                ></span>
                <span className="w-1 h-1 bg-white/70 rounded-full animate-bounce"></span>
              </span>
            </div>
          ) : (
            <input
              type="text"
              value={inputText}
              onChange={(e) => setInputText(e.target.value)}
              onKeyDown={(e) => e.key === "Enter" && handleSend()}
              placeholder={
                isRecording ? "Listening..." : "Initialize command sequence..."
              }
              aria-label="Command input"
              className="w-full bg-transparent border-none outline-none text-white px-2 placeholder:text-white/30 text-sm font-medium focus-visible:ring-0"
              disabled={isRecording}
            />
          )}
        </div>

        <button
          type="button"
          onClick={handleSend}
          disabled={!inputText.trim() && !isTranscribing}
          aria-label="Send command"
          className={`relative z-10 w-10 h-10 sm:w-11 sm:h-11 flex items-center justify-center rounded-xl border transition-all duration-200 press-attenuation focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-white/20 shrink-0 ${
            inputText.trim()
              ? "bg-white text-black border-white hover:bg-white/90 active:scale-95 shadow-md"
              : "bg-white/5 border-white/10 text-white/30 cursor-not-allowed"
          }`}
          title="Send command"
        >
          <Send className="w-5 h-5" aria-hidden="true" />
        </button>
      </div>
    </div>
  );
});
