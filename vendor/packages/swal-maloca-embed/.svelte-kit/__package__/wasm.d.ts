export interface BacklogItem {
    id: string;
    title: string;
    status: string;
}
export interface PackInfo {
    features: number;
    decisions: number;
}
/**
 * Normalizes the backlog using WASM, or falls back to TS if WASM is unavailable.
 */
export declare function normalizeBacklog(backlogRaw: any, appId: string): Promise<BacklogItem[]>;
/**
 * Normalizes the pack info.
 * Note: normalize_pack operation does not exist currently in maloca-core (Rust).
 * If it gets added, it will be called via WASM. Otherwise, we use TS fallback.
 */
export declare function normalizePack(packRaw: any): Promise<PackInfo | null>;
