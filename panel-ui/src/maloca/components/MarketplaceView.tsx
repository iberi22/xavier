import React, { useEffect, useState } from "react";
import {
  malocaApi,
  MarketListing,
  MarketReceipt,
  MarketConsentRecord,
} from "../api";
import {
  ShoppingBag,
  Tag,
  Shield,
  FileCheck,
  Plus,
  RefreshCw,
  CheckCircle2,
  AlertCircle,
  XCircle,
  Layers,
} from "lucide-react";

export function MarketplaceView() {
  const [subView, setSubView] = useState<"catalogue" | "seller" | "consent">("catalogue");

  // Catalogue State
  const [listings, setListings] = useState<MarketListing[]>([]);
  const [receipts, setReceipts] = useState<MarketReceipt[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [actionMessage, setActionMessage] = useState<string | null>(null);

  // Buy Flow Modal / Status
  const [buyingId, setBuyingId] = useState<string | null>(null);

  // Seller Form State
  const [sellerId, setSellerId] = useState("local-node");
  const [newTitle, setNewTitle] = useState("");
  const [newDesc, setNewDesc] = useState("");
  const [newPrice, setNewPrice] = useState(10);
  const [newLicense, setNewLicense] = useState("cc-by-4.0");
  const [newTermsVersion, setNewTermsVersion] = useState("v1.0");
  const [isPublishing, setIsPublishing] = useState(false);

  // Signing is not available in panel-ui yet (requires client-side ed25519 keypair signing)
  const isSigningAvailable = false;

  // Consent Screen State
  const [consentNodeId, setConsentNodeId] = useState("local-node");
  const [activeConsentNodeId, setActiveConsentNodeId] = useState("local-node");
  const [consentState, setConsentState] = useState<"off" | "local" | "shared" | "shared_sensitive">("off");
  const [consentTermsVer, setConsentTermsVer] = useState("v1.0");
  const [consentVer, setConsentVer] = useState("v1.0");
  const [consentRecords, setConsentRecords] = useState<MarketConsentRecord[]>([]);
  const [isSavingConsent, setIsSavingConsent] = useState(false);

  const fetchMarketData = async (targetNodeId = activeConsentNodeId) => {
    setLoading(true);
    setError(null);
    try {
      const data = await malocaApi.getMarketplaceListings();
      setListings(data);
      const consentData = await malocaApi.getMarketplaceConsent();
      if (Array.isArray(consentData)) {
        setConsentRecords(consentData);
        const current = consentData.find((r) => r.node_id === targetNodeId);
        if (current) {
          setConsentState(current.state);
          setConsentTermsVer(current.terms_version || "v1.0");
          setConsentVer(current.consent_version || "v1.0");
        }
      }
      const myReceipts = await malocaApi.getMarketplaceReceipts(targetNodeId).catch(() => []);
      setReceipts(myReceipts);
    } catch (err: unknown) {
      const msg = err instanceof Error ? err.message : "Failed to load marketplace";
      setError(msg);
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    void fetchMarketData(activeConsentNodeId);
  }, [activeConsentNodeId]);

  const handleBuy = async (listing: MarketListing) => {
    setBuyingId(listing.id);
    setError(null);
    setActionMessage(null);
    try {
      const receipt = await malocaApi.buyMarketplaceListing({
        listing_id: listing.id,
        buyer_id: activeConsentNodeId,
      });
      setActionMessage(`Purchase successful! Receipt ID: ${receipt.id.slice(0, 8)}... (${receipt.amount_credits} credits)`);
      await fetchMarketData(activeConsentNodeId);
    } catch (err: unknown) {
      const msg = err instanceof Error ? err.message : "Purchase failed";
      setError(msg);
    } finally {
      setBuyingId(null);
    }
  };

  const handleCreateListing = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!isSigningAvailable) {
      setError("Signing required to publish listings.");
      return;
    }
    setIsPublishing(true);
    setError(null);
    setActionMessage(null);
    try {
      const created = await malocaApi.createMarketplaceListing({
        seller_id: sellerId,
        title: newTitle,
        description: newDesc,
        price_credits: Number(newPrice),
        license: newLicense,
        terms_version: newTermsVersion,
      });
      setActionMessage(`Listing created: "${created.title}"`);
      setNewTitle("");
      setNewDesc("");
      await fetchMarketData(activeConsentNodeId);
      setSubView("catalogue");
    } catch (err: unknown) {
      const msg = err instanceof Error ? err.message : "Failed to create listing";
      setError(msg);
    } finally {
      setIsPublishing(false);
    }
  };

  const handleRevokeListing = async (id: string) => {
    if (!isSigningAvailable) {
      setError("Signing required to revoke listings.");
      return;
    }
    if (typeof window !== "undefined" && !window.confirm("Are you sure you want to revoke this listing?")) {
      return;
    }
    setError(null);
    setActionMessage(null);
    try {
      await malocaApi.revokeMarketplaceListing(id);
      setActionMessage(`Listing revoked successfully.`);
      await fetchMarketData(activeConsentNodeId);
    } catch (err: unknown) {
      const msg = err instanceof Error ? err.message : "Failed to revoke listing";
      setError(msg);
    }
  };

  const handleSaveConsent = async (e: React.FormEvent) => {
    e.preventDefault();
    setIsSavingConsent(true);
    setError(null);
    setActionMessage(null);
    try {
      const updated = await malocaApi.setMarketplaceConsent({
        node_id: consentNodeId,
        state: consentState,
        terms_version: consentTermsVer,
        consent_version: consentVer,
      });
      setActionMessage(`Consent state updated to: ${updated.state.toUpperCase()}`);
      await fetchMarketData();
    } catch (err: unknown) {
      const msg = err instanceof Error ? err.message : "Failed to update consent";
      setError(msg);
    } finally {
      setIsSavingConsent(false);
    }
  };

  return (
    <div className="space-y-6" data-testid="marketplace-view">
      {/* Sub-navigation Controls */}
      <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-4 pb-4 border-b border-white/10">
        <div className="flex items-center gap-2">
          <button
            type="button"
            onClick={() => setSubView("catalogue")}
            className={`px-3 py-1.5 rounded-lg text-xs font-mono transition-colors ${
              subView === "catalogue"
                ? "bg-emerald-500/20 text-emerald-300 border border-emerald-500/40"
                : "text-white/60 hover:text-white hover:bg-white/5"
            }`}
          >
            Buyer Catalogue ({listings.filter((l) => !l.revoked_at_ms).length})
          </button>
          <button
            type="button"
            onClick={() => setSubView("seller")}
            className={`px-3 py-1.5 rounded-lg text-xs font-mono transition-colors ${
              subView === "seller"
                ? "bg-emerald-500/20 text-emerald-300 border border-emerald-500/40"
                : "text-white/60 hover:text-white hover:bg-white/5"
            }`}
          >
            Seller Console
          </button>
          <button
            type="button"
            onClick={() => setSubView("consent")}
            className={`px-3 py-1.5 rounded-lg text-xs font-mono transition-colors ${
              subView === "consent"
                ? "bg-emerald-500/20 text-emerald-300 border border-emerald-500/40"
                : "text-white/60 hover:text-white hover:bg-white/5"
            }`}
          >
            Consent Screen ({consentState.toUpperCase()})
          </button>
        </div>

        <button
          type="button"
          onClick={() => void fetchMarketData()}
          disabled={loading}
          className="flex items-center gap-1.5 text-xs font-mono text-white/50 hover:text-white transition-colors"
          title="Refresh Data"
        >
          <RefreshCw size={13} className={loading ? "animate-spin" : ""} />
          <span>Sync</span>
        </button>
      </div>

      {/* Action Banners */}
      {actionMessage && (
        <div className="p-3 bg-emerald-950/40 border border-emerald-600/40 rounded-xl text-emerald-300 text-xs font-mono flex items-center gap-2">
          <CheckCircle2 size={16} className="text-emerald-400 shrink-0" />
          <span>{actionMessage}</span>
        </div>
      )}

      {error && (
        <div className="p-3 bg-rose-950/40 border border-rose-600/40 rounded-xl text-rose-300 text-xs font-mono flex items-center gap-2">
          <AlertCircle size={16} className="text-rose-400 shrink-0" />
          <span>{error}</span>
        </div>
      )}

      {/* --- SUBVIEW: BUYER CATALOGUE --- */}
      {subView === "catalogue" && (
        <div className="space-y-4">
          <div className="flex items-center justify-between">
            <h2 className="text-sm font-semibold text-white/80 uppercase tracking-wider font-mono">
              Available Data Packages
            </h2>
            <span className="text-xs font-mono text-white/40">Settlement: Off-Chain Credits</span>
          </div>

          {loading ? (
            <div className="flex items-center justify-center h-36 text-white/40 font-mono text-xs">
              Loading marketplace listings...
            </div>
          ) : listings.length === 0 ? (
            <div className="p-8 text-center border border-dashed border-white/10 rounded-2xl">
              <ShoppingBag className="w-8 h-8 text-white/20 mx-auto mb-2" />
              <p className="text-sm text-white/60 font-mono">No listings available in this network yet.</p>
              <button
                type="button"
                onClick={() => setSubView("seller")}
                className="mt-3 px-3 py-1.5 bg-emerald-500/20 hover:bg-emerald-500/30 text-emerald-300 border border-emerald-500/40 rounded-lg text-xs font-mono transition-colors"
              >
                Create First Listing
              </button>
            </div>
          ) : (
            <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
              {listings.map((item) => {
                const isRevoked = Boolean(item.revoked_at_ms);
                const hasPurchased = receipts.some((r) => r.listing_id === item.id);

                return (
                  <div
                    key={item.id}
                    className={`glass-panel p-5 rounded-2xl border transition-all ${
                      isRevoked
                        ? "border-rose-900/40 bg-black/40 opacity-60"
                        : "border-white/10 bg-[#0a0a0a] hover:border-emerald-500/30"
                    }`}
                  >
                    <div className="flex items-start justify-between gap-2 mb-2">
                      <h3 className="text-base font-semibold text-white tracking-tight">{item.title}</h3>
                      <span
                        className={`text-[11px] font-mono px-2 py-0.5 rounded-full border ${
                          isRevoked
                            ? "border-rose-500/40 text-rose-400 bg-rose-500/10"
                            : "border-emerald-500/40 text-emerald-300 bg-emerald-500/10 font-bold"
                        }`}
                      >
                        {item.price_credits} CREDITS
                      </span>
                    </div>

                    <p className="text-xs text-white/60 mb-4 line-clamp-2">
                      {item.description || "No description provided."}
                    </p>

                    <div className="flex flex-wrap items-center gap-2 text-[10px] font-mono text-white/50 mb-4 border-t border-white/5 pt-3">
                      <span className="flex items-center gap-1">
                        <Tag size={12} className="text-emerald-400" />
                        License: {item.license}
                      </span>
                      <span>·</span>
                      <span>Terms: {item.terms_version}</span>
                      <span>·</span>
                      <span>Seller: {item.seller_id.slice(0, 8)}...</span>
                    </div>

                    <div className="flex items-center justify-between pt-2">
                      {isRevoked ? (
                        <span className="text-xs font-mono text-rose-400 flex items-center gap-1">
                          <XCircle size={14} /> Revoked by seller
                        </span>
                      ) : hasPurchased ? (
                        <span className="text-xs font-mono text-emerald-400 flex items-center gap-1">
                          <CheckCircle2 size={14} /> Purchased
                        </span>
                      ) : (
                        <button
                          type="button"
                          onClick={() => void handleBuy(item)}
                          disabled={buyingId === item.id}
                          className="px-4 py-1.5 bg-emerald-500/20 hover:bg-emerald-500/30 text-emerald-300 border border-emerald-500/40 rounded-xl text-xs font-mono font-medium transition-all"
                        >
                          {buyingId === item.id ? "Processing..." : `Buy for ${item.price_credits} credits`}
                        </button>
                      )}
                    </div>
                  </div>
                );
              })}
            </div>
          )}

          {receipts.length > 0 && (
            <div className="mt-8 pt-6 border-t border-white/10">
              <h3 className="text-xs font-mono text-white/50 uppercase tracking-wider mb-3">
                My Purchases & Receipts ({receipts.length})
              </h3>
              <div className="space-y-2">
                {receipts.map((r) => (
                  <div
                    key={r.id}
                    className="p-3 bg-white/5 rounded-xl border border-white/5 flex items-center justify-between text-xs font-mono"
                  >
                    <div>
                      <span className="text-white/80 font-bold">Listing ID:</span> {r.listing_id}
                      <span className="text-white/40 ml-2">Receipt: {r.id.slice(0, 8)}...</span>
                    </div>
                    <span className="text-emerald-400 font-bold">{r.amount_credits} CREDITS</span>
                  </div>
                ))}
              </div>
            </div>
          )}
        </div>
      )}

      {/* --- SUBVIEW: SELLER CONSOLE --- */}
      {subView === "seller" && (
        <div className="space-y-6">
          <div className="glass-panel p-6 rounded-2xl border border-white/10 bg-[#0a0a0a]">
            <h2 className="text-base font-semibold text-white mb-1">Publish New Dataset / Package</h2>
            <p className="text-xs text-white/50 mb-6 font-mono">
              Listings are recorded on SQLite v2 and broadcast to authorized mesh buyers.
            </p>

            <form onSubmit={handleCreateListing} className="space-y-4 max-w-xl">
              <div>
                <label className="block text-xs font-mono text-white/70 mb-1">Seller Identity / Node ID</label>
                <input
                  type="text"
                  required
                  value={sellerId}
                  onChange={(e) => setSellerId(e.target.value)}
                  className="w-full bg-black/60 border border-white/10 rounded-xl px-3 py-2 text-xs font-mono text-white focus:outline-none focus:border-emerald-500"
                />
              </div>

              <div>
                <label className="block text-xs font-mono text-white/70 mb-1">Listing Title</label>
                <input
                  type="text"
                  required
                  placeholder="e.g. Colombian Biology QA Dataset (ICFES High Quality)"
                  value={newTitle}
                  onChange={(e) => setNewTitle(e.target.value)}
                  className="w-full bg-black/60 border border-white/10 rounded-xl px-3 py-2 text-xs font-mono text-white focus:outline-none focus:border-emerald-500"
                />
              </div>

              <div>
                <label className="block text-xs font-mono text-white/70 mb-1">Description</label>
                <textarea
                  rows={3}
                  placeholder="Package contents, schema information, provenance details..."
                  value={newDesc}
                  onChange={(e) => setNewDesc(e.target.value)}
                  className="w-full bg-black/60 border border-white/10 rounded-xl px-3 py-2 text-xs font-mono text-white focus:outline-none focus:border-emerald-500 resize-none"
                />
              </div>

              <div className="grid grid-cols-1 sm:grid-cols-3 gap-3">
                <div>
                  <label className="block text-xs font-mono text-white/70 mb-1">Price (Credits)</label>
                  <input
                    type="number"
                    min={0}
                    required
                    value={newPrice}
                    onChange={(e) => setNewPrice(Number(e.target.value))}
                    className="w-full bg-black/60 border border-white/10 rounded-xl px-3 py-2 text-xs font-mono text-white focus:outline-none focus:border-emerald-500"
                  />
                </div>

                <div>
                  <label className="block text-xs font-mono text-white/70 mb-1">License</label>
                  <select
                    value={newLicense}
                    onChange={(e) => setNewLicense(e.target.value)}
                    className="w-full bg-black/60 border border-white/10 rounded-xl px-3 py-2 text-xs font-mono text-white focus:outline-none focus:border-emerald-500"
                  >
                    <option value="cc-by-4.0">cc-by-4.0</option>
                    <option value="cc0-1.0">cc0-1.0</option>
                    <option value="mit">mit</option>
                    <option value="apache-2.0">apache-2.0</option>
                    <option value="proprietary-research">proprietary-research</option>
                  </select>
                </div>

                <div>
                  <label className="block text-xs font-mono text-white/70 mb-1">Terms Version</label>
                  <input
                    type="text"
                    value={newTermsVersion}
                    onChange={(e) => setNewTermsVersion(e.target.value)}
                    className="w-full bg-black/60 border border-white/10 rounded-xl px-3 py-2 text-xs font-mono text-white focus:outline-none focus:border-emerald-500"
                  />
                </div>
              </div>

              <div className="flex items-center gap-3 mt-2">
                <button
                  type="submit"
                  disabled={isPublishing || !isSigningAvailable}
                  className={`flex items-center gap-2 px-5 py-2.5 font-semibold rounded-xl text-xs font-mono transition-all ${
                    !isSigningAvailable
                      ? "bg-white/10 text-white/40 cursor-not-allowed border border-white/10"
                      : "bg-emerald-500 text-black hover:bg-emerald-400 shadow-[0_0_15px_rgba(57,255,20,0.3)]"
                  }`}
                  title={!isSigningAvailable ? "Signing required (ed25519 publisher signature)" : undefined}
                >
                  <Plus size={16} />
                  {isPublishing ? "Publishing..." : "Publish Listing"}
                </button>
                {!isSigningAvailable && (
                  <span className="text-[11px] font-mono text-amber-400/80 bg-amber-500/10 px-2.5 py-1 rounded-lg border border-amber-500/20">
                    Signing required (client-side Ed25519 signature)
                  </span>
                )}
              </div>
            </form>
          </div>

          {/* Manage Published Listings */}
          <div>
            <h3 className="text-xs font-mono text-white/50 uppercase tracking-wider mb-3">
              Published Listings Management
            </h3>
            <div className="space-y-2">
              {listings.map((l) => (
                <div
                  key={l.id}
                  className="p-3 bg-[#0a0a0a] rounded-xl border border-white/10 flex items-center justify-between text-xs font-mono"
                >
                  <div>
                    <span className="text-white font-semibold">{l.title}</span>
                    <span className="text-white/40 ml-2 font-mono text-[11px]">{l.id}</span>
                  </div>
                  <div className="flex items-center gap-3">
                    <span className="text-emerald-400 font-bold">{l.price_credits} credits</span>
                    {!l.revoked_at_ms ? (
                      <button
                        type="button"
                        disabled={!isSigningAvailable}
                        onClick={() => void handleRevokeListing(l.id)}
                        className={`px-2.5 py-1 rounded-lg text-[11px] transition-colors ${
                          !isSigningAvailable
                            ? "bg-white/5 text-white/30 border border-white/5 cursor-not-allowed"
                            : "bg-rose-500/10 hover:bg-rose-500/20 text-rose-400 border border-rose-500/30"
                        }`}
                        title={!isSigningAvailable ? "Signing required to revoke" : undefined}
                      >
                        Revoke
                      </button>
                    ) : (
                      <span className="text-rose-400 text-[11px]">Revoked</span>
                    )}
                  </div>
                </div>
              ))}
            </div>
          </div>
        </div>
      )}

      {/* --- SUBVIEW: CONSENT SCREEN --- */}
      {subView === "consent" && (
        <div className="space-y-6">
          <div className="glass-panel p-6 rounded-2xl border border-white/10 bg-[#0a0a0a]">
            <div className="flex items-center gap-2 mb-2">
              <Shield className="w-5 h-5 text-emerald-400" />
              <h2 className="text-base font-semibold text-white">Data Node Participation Consent</h2>
            </div>
            <p className="text-xs text-white/50 mb-6 font-mono leading-relaxed">
              Define the boundary for telemetry, dataset sharing and P2P exchange for this node.
              State changes require affirmative terms and version acknowledgment.
            </p>

            <form onSubmit={handleSaveConsent} className="space-y-5 max-w-xl">
              <div>
                <label className="block text-xs font-mono text-white/70 mb-1">Target Node ID</label>
                <div className="flex gap-2">
                  <input
                    type="text"
                    required
                    value={consentNodeId}
                    onChange={(e) => setConsentNodeId(e.target.value)}
                    onBlur={() => setActiveConsentNodeId(consentNodeId.trim())}
                    onKeyDown={(e) => {
                      if (e.key === "Enter") {
                        e.preventDefault();
                        setActiveConsentNodeId(consentNodeId.trim());
                      }
                    }}
                    className="flex-1 bg-black/60 border border-white/10 rounded-xl px-3 py-2 text-xs font-mono text-white focus:outline-none focus:border-emerald-500"
                  />
                  <button
                    type="button"
                    onClick={() => setActiveConsentNodeId(consentNodeId.trim())}
                    className="px-3 py-2 bg-white/10 hover:bg-white/20 text-white rounded-xl text-xs font-mono transition-colors"
                  >
                    Load Node
                  </button>
                </div>
              </div>

              <div>
                <label className="block text-xs font-mono text-white/70 mb-2">Consent Tier</label>
                <div className="grid grid-cols-1 sm:grid-cols-2 gap-3">
                  {[
                    { id: "off", label: "Off", desc: "No telemetry or data leaves node." },
                    { id: "local", label: "Local Only", desc: "Collected locally, never leaves node." },
                    { id: "shared", label: "Shared", desc: "Sanitized data may leave node for consensus." },
                    { id: "shared_sensitive", label: "Shared Sensitive", desc: "Full mesh data collaboration." },
                  ].map((tier) => (
                    <button
                      key={tier.id}
                      type="button"
                      onClick={() => setConsentState(tier.id as any)}
                      className={`p-3 rounded-xl border text-left transition-all ${
                        consentState === tier.id
                          ? "border-emerald-500/60 bg-emerald-500/10 text-white shadow-[inset_0_0_8px_rgba(57,255,20,0.1)]"
                          : "border-white/10 bg-black/40 text-white/60 hover:border-white/20"
                      }`}
                    >
                      <div className="font-mono text-xs font-bold capitalize text-white mb-0.5">
                        {tier.label}
                      </div>
                      <div className="text-[10px] text-white/50 leading-tight">{tier.desc}</div>
                    </button>
                  ))}
                </div>
              </div>

              <div className="grid grid-cols-1 sm:grid-cols-2 gap-3">
                <div>
                  <label className="block text-xs font-mono text-white/70 mb-1">Terms Version</label>
                  <input
                    type="text"
                    required
                    aria-label="Terms Version"
                    value={consentTermsVer}
                    onChange={(e) => setConsentTermsVer(e.target.value)}
                    className="w-full bg-black/60 border border-white/10 rounded-xl px-3 py-2 text-xs font-mono text-white focus:outline-none focus:border-emerald-500"
                  />
                </div>

                <div>
                  <label className="block text-xs font-mono text-white/70 mb-1">Consent Version</label>
                  <input
                    type="text"
                    required
                    aria-label="Consent Version"
                    value={consentVer}
                    onChange={(e) => setConsentVer(e.target.value)}
                    className="w-full bg-black/60 border border-white/10 rounded-xl px-3 py-2 text-xs font-mono text-white focus:outline-none focus:border-emerald-500"
                  />
                </div>
              </div>

              <button
                type="submit"
                disabled={isSavingConsent}
                className="flex items-center gap-2 px-5 py-2.5 bg-emerald-500 text-black font-semibold rounded-xl text-xs font-mono hover:bg-emerald-400 transition-all shadow-[0_0_15px_rgba(57,255,20,0.3)]"
              >
                <FileCheck size={16} />
                {isSavingConsent ? "Saving..." : "Apply Node Consent"}
              </button>
            </form>
          </div>

          {/* Historical / Active Node Consents */}
          <div>
            <h3 className="text-xs font-mono text-white/50 uppercase tracking-wider mb-3">
              Recorded Node Consents
            </h3>
            <div className="space-y-2">
              {consentRecords.map((c) => (
                <div
                  key={c.node_id}
                  className="p-3 bg-[#0a0a0a] rounded-xl border border-white/10 flex items-center justify-between text-xs font-mono"
                >
                  <div className="flex items-center gap-2">
                    <span className="text-white font-semibold">{c.node_id}</span>
                    <span className="text-white/40 text-[11px]">Terms: {c.terms_version}</span>
                  </div>
                  <span
                    className={`px-2 py-0.5 rounded-full text-[10px] font-bold uppercase border ${
                      c.state === "off"
                        ? "border-rose-500/30 text-rose-400 bg-rose-500/10"
                        : "border-emerald-500/30 text-emerald-400 bg-emerald-500/10"
                    }`}
                  >
                    {c.state}
                  </span>
                </div>
              ))}
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
export default MarketplaceView;
