import React, { useEffect, useState } from "react";
import { ShoppingBag, ShieldCheck, ArrowRight, RefreshCw } from "lucide-react";
import { malocaApi, MarketListing, MarketConsentRecord } from "../maloca/api";

export interface MarketplaceStatusCardProps {
  onOpenMarketplace?: () => void;
  className?: string;
}

export function MarketplaceStatusCard({
  onOpenMarketplace,
  className = "",
}: MarketplaceStatusCardProps) {
  const [activeListingsCount, setActiveListingsCount] = useState<number | null>(null);
  const [consentStatus, setConsentStatus] = useState<string>("off");
  const [loading, setLoading] = useState(true);

  const fetchStatus = async () => {
    setLoading(true);
    try {
      const listings = await malocaApi.getMarketplaceListings().catch(() => []);
      const active = listings.filter((l) => !l.revoked_at_ms).length;
      setActiveListingsCount(active);

      const consent = await malocaApi.getMarketplaceConsent().catch(() => []);
      if (Array.isArray(consent) && consent.length > 0) {
        setConsentStatus(consent[0].state || "off");
      } else if (!Array.isArray(consent) && consent && "state" in consent) {
        setConsentStatus((consent as MarketConsentRecord).state);
      }
    } catch {
      setActiveListingsCount(0);
      setConsentStatus("off");
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    void fetchStatus();
  }, []);

  const handleOpen = () => {
    if (onOpenMarketplace) {
      onOpenMarketplace();
    } else {
      window.location.hash = "#/maloca";
    }
  };

  return (
    <div
      data-testid="marketplace-status-card"
      className={`glass-panel p-4 rounded-xl border border-white/10 bg-[#0a0a0a]/90 text-white font-sans ${className}`}
    >
      <div className="flex items-center justify-between mb-3">
        <div className="flex items-center gap-2">
          <ShoppingBag className="w-4 h-4 text-emerald-400" />
          <span className="text-xs font-mono font-bold tracking-wider uppercase text-white/90">
            Marketplace v2
          </span>
        </div>
        <button
          type="button"
          onClick={() => void fetchStatus()}
          className="text-white/40 hover:text-white transition-colors"
          title="Refresh Status"
          aria-label="Refresh Status"
        >
          <RefreshCw size={12} className={loading ? "animate-spin" : ""} />
        </button>
      </div>

      <div className="grid grid-cols-2 gap-2 mb-3">
        <div className="p-2 rounded-lg bg-black/40 border border-white/5">
          <div className="text-[10px] font-mono text-white/50 uppercase">Active Listings</div>
          <div className="text-lg font-bold text-white font-mono">
            {loading ? "..." : (activeListingsCount ?? 0)}
          </div>
        </div>
        <div className="p-2 rounded-lg bg-black/40 border border-white/5">
          <div className="text-[10px] font-mono text-white/50 uppercase">Consent Tier</div>
          <div className="text-xs font-bold font-mono uppercase text-emerald-400 mt-1">
            {loading ? "..." : consentStatus}
          </div>
        </div>
      </div>

      <button
        type="button"
        onClick={handleOpen}
        className="w-full flex items-center justify-between px-3 py-2 bg-emerald-500/10 hover:bg-emerald-500/20 text-emerald-300 border border-emerald-500/30 rounded-lg text-xs font-mono transition-all group"
      >
        <span>Open Maloca Console</span>
        <ArrowRight size={13} className="group-hover:translate-x-0.5 transition-transform" />
      </button>
    </div>
  );
}
export default MarketplaceStatusCard;
