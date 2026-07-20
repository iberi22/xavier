import {
  AlertCircle,
  CheckCircle2,
  Eye,
  EyeOff,
  Loader2,
  TestTube,
  Trash2,
} from "lucide-react";
import React from "react";

interface ApiKeyInputProps {
  label: string;
  value: string;
  onChange: (value: string) => void;
  onTest: () => Promise<void>;
  onRemove: () => void;
}

export function ApiKeyInput({
  label,
  value,
  onChange,
  onTest,
  onRemove,
}: ApiKeyInputProps) {
  const [show, setShow] = React.useState(false);
  const [testing, setTesting] = React.useState(false);
  const [testResult, setTestResult] = React.useState<
    "none" | "success" | "error"
  >("none");
  const inputId = React.useId();

  const handleTest = async () => {
    setTesting(true);
    setTestResult("none");
    try {
      await onTest();
      setTestResult("success");
    } catch (_e) {
      setTestResult("error");
    } finally {
      setTesting(false);
    }
  };

  return (
    <div className="space-y-2">
      <label
        htmlFor={inputId}
        className="text-[10px] uppercase text-white/50 tracking-widest block"
      >
        {label}
      </label>
      <div className="flex gap-2">
        <div className="relative flex-1 group">
          <input
            id={inputId}
            type={show ? "text" : "password"}
            value={value}
            onChange={(e) => onChange(e.target.value)}
            className="w-full bg-[#050505]/80 border border-white/10 focus:border-[#39ff14]/50 rounded-xl px-4 py-3 text-sm transition-all outline-none"
            placeholder="sk-...."
          />
          <button
            type="button"
            onClick={() => setShow(!show)}
            aria-label={show ? "Hide API key" : "Show API key"}
            title={show ? "Hide API key" : "Show API key"}
            className="absolute right-4 top-1/2 -translate-y-1/2 text-white/20 hover:text-white/60 transition-colors focus-visible:ring-2 focus-visible:ring-[#39ff14]/50 rounded"
          >
            {show ? (
              <EyeOff className="w-4 h-4" aria-hidden="true" />
            ) : (
              <Eye className="w-4 h-4" aria-hidden="true" />
            )}
          </button>
        </div>

        <button
          type="button"
          onClick={handleTest}
          disabled={testing || !value}
          className={`flex items-center gap-2 px-4 py-3 rounded-xl text-xs font-bold tracking-wider transition-all
            ${
              testResult === "success"
                ? "bg-[#39ff14]/10 text-[#39ff14] border border-[#39ff14]/20"
                : testResult === "error"
                  ? "bg-red-500/10 text-red-400 border border-red-500/20"
                  : "bg-white/5 hover:bg-white/10 text-white/70 border border-white/5"
            }
          `}
        >
          {testing ? (
            <Loader2 className="w-4 h-4 animate-spin" />
          ) : testResult === "success" ? (
            <CheckCircle2 className="w-4 h-4" />
          ) : testResult === "error" ? (
            <AlertCircle className="w-4 h-4" />
          ) : (
            <TestTube className="w-4 h-4" />
          )}
          {testing
            ? "Testing..."
            : testResult === "success"
              ? "Valid"
              : testResult === "error"
                ? "Failed"
                : "Test"}
        </button>

        <button
          type="button"
          onClick={onRemove}
          aria-label="Remove API key"
          className="p-3 bg-white/5 hover:bg-red-500/10 hover:text-red-400 border border-white/5 rounded-xl transition-all focus-visible:ring-2 focus-visible:ring-[#39ff14]/50"
          title="Remove Key"
        >
          <Trash2 className="w-4 h-4" aria-hidden="true" />
        </button>
      </div>
    </div>
  );
}
