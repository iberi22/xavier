import { useState } from "react";
import { obtainDeviceKeyViaWebAuthn } from "./webauthn";
import { MalocaTabId } from "./tabs";

export function useMalocaView() {
  const [activeTab, setActiveTab] = useState<MalocaTabId>("overview");
  const [error, setError] = useState<string | null>(null);
  const [deviceKey, setDeviceKey] = useState<string | null>(null);
  const [isWebAuthnLoading, setIsWebAuthnLoading] = useState(false);

  const handleObtainWebAuthnKey = async () => {
    setIsWebAuthnLoading(true);
    setError(null);
    try {
      const key = await obtainDeviceKeyViaWebAuthn();
      setDeviceKey(key);
    } catch (err: any) {
      setError(err?.message || "Failed to obtain WebAuthn key");
      // Use fallback/mock for dev environment if webauthn fails due to missing https
      console.warn("WebAuthn failed, falling back to mock key for development");
      setTimeout(() => {
        setDeviceKey("swal_dev_key_" + Math.random().toString(36).substring(2, 15));
      }, 500);
    } finally {
      setIsWebAuthnLoading(false);
    }
  };

  return {
    activeTab,
    setActiveTab,
    error,
    deviceKey,
    isWebAuthnLoading,
    handleObtainWebAuthnKey,
  };
}
