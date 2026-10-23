import { useEffect, useState } from "react";
import { obtainDeviceKeyViaWebAuthn } from "./webauthn";
import { MalocaTabId } from "./tabs";
import { probeMarketplaceAvailable } from "./api";

export function useMalocaView() {
  const [activeTab, setActiveTab] = useState<MalocaTabId>("overview");
  const [error, setError] = useState<string | null>(null);
  const [deviceKey, setDeviceKey] = useState<string | null>(null);
  const [isWebAuthnLoading, setIsWebAuthnLoading] = useState(false);
  const [marketplaceAvailable, setMarketplaceAvailable] = useState(false);

  useEffect(() => {
    let cancelled = false;
    async function checkMarketplace() {
      const available = await probeMarketplaceAvailable();
      if (!cancelled) {
        setMarketplaceAvailable(available);
      }
    }
    void checkMarketplace();
    return () => {
      cancelled = true;
    };
  }, []);

  const handleObtainWebAuthnKey = async () => {
    setIsWebAuthnLoading(true);
    setError(null);
    try {
      const key = await obtainDeviceKeyViaWebAuthn();
      setDeviceKey(key);
    } catch (err: any) {
      // No fallback key: a failed WebAuthn attempt must leave deviceKey null.
      setError(err?.message || "Failed to obtain WebAuthn key");
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
    marketplaceAvailable,
    handleObtainWebAuthnKey,
  };
}
