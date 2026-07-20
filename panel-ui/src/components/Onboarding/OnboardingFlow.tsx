import { invoke } from "@tauri-apps/api/core";
import { useState } from "react";
import { AuthStep } from "./AuthStep";
import { HardwareStep } from "./HardwareStep";
import { IntegrationsStep } from "./IntegrationsStep";
import { SystemScanStep } from "./SystemScanStep";
import { WelcomeStep } from "./WelcomeStep";

export type SystemInfo = {
  total_ram_gb: number;
  cpu_cores: number;
  has_gpu: boolean;
  openclaw_running: boolean;
  hermes_running: boolean;
};

export type InitialConfig = {
  telegram_token: string;
  use_gpu_model: boolean;
};

export function OnboardingFlow({ onComplete }: { onComplete: () => void }) {
  const [step, setStep] = useState(0);
  const [systemInfo, setSystemInfo] = useState<SystemInfo | null>(null);
  const [config, setConfig] = useState<InitialConfig>({
    telegram_token: "",
    use_gpu_model: false,
  });
  const [isSaving, setIsSaving] = useState(false);

  const handleNext = () => setStep((s) => s + 1);

  const handleComplete = async () => {
    try {
      setIsSaving(true);
      await invoke("save_initial_config", { config });
      localStorage.setItem("xavier_onboarding_completed", "true");
      onComplete();
    } catch (e) {
      console.error("Error saving config", e);
    } finally {
      setIsSaving(false);
    }
  };

  const renderStep = () => {
    switch (step) {
      case 0:
        return <WelcomeStep onNext={handleNext} />;
      case 1:
        return (
          <SystemScanStep
            onNext={(info) => {
              setSystemInfo(info);
              setConfig((prev) => ({ ...prev, use_gpu_model: info.has_gpu }));
              handleNext();
            }}
          />
        );
      case 2:
        return (
          <HardwareStep
            systemInfo={systemInfo!}
            useGpu={config.use_gpu_model}
            onChangeUseGpu={(val) =>
              setConfig((prev) => ({ ...prev, use_gpu_model: val }))
            }
            onNext={handleNext}
          />
        );
      case 3:
        return (
          <IntegrationsStep
            token={config.telegram_token}
            onChangeToken={(val) =>
              setConfig((prev) => ({ ...prev, telegram_token: val }))
            }
            onComplete={handleNext}
            isSaving={isSaving}
          />
        );
      case 4:
        return <AuthStep onSkip={handleComplete} onComplete={handleComplete} />;
      default:
        return null;
    }
  };

  return (
    <div className="fixed inset-0 bg-[#0a0a0a] text-emerald-400 flex flex-col items-center justify-center p-6 z-50 font-mono">
      <div className="w-full max-w-2xl bg-black border border-emerald-900 rounded-lg shadow-[0_0_20px_rgba(16,185,129,0.15)] overflow-hidden">
        {/* Progress Bar */}
        <div className="h-1 w-full bg-neutral-900">
          <div
            className="h-full bg-emerald-500 transition-all duration-500 ease-in-out"
            style={{ width: `${((step + 1) / 5) * 100}%` }}
          />
        </div>
        <div className="p-8">{renderStep()}</div>
      </div>
    </div>
  );
}
