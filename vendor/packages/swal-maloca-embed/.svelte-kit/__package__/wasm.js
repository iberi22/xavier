// singleton for maloca-wasm with TS fallback
let wasmClientPromise = null;
async function getWasmClient() {
    if (wasmClientPromise)
        return wasmClientPromise;
    wasmClientPromise = (async () => {
        try {
            // Dynamic import with @ts-ignore to allow standalone build if dependency is resolved at runtime
            // @ts-ignore
            const { MalocaWasmClient } = await import("@swal/maloca-wasm/main");
            return MalocaWasmClient;
        }
        catch (e) {
            console.warn("maloca-wasm is not available, using TS fallback", e);
            return null;
        }
    })();
    return wasmClientPromise;
}
/**
 * Normalizes the backlog using WASM, or falls back to TS if WASM is unavailable.
 */
export async function normalizeBacklog(backlogRaw, appId) {
    try {
        const client = await getWasmClient();
        if (client && typeof client.apply === "function") {
            // Call WASM: MalocaWasmClient.apply("normalize_backlog", {json, app_id})
            const result = await client.apply("normalize_backlog", { json: backlogRaw, app_id: appId });
            if (result) {
                return result;
            }
        }
    }
    catch (err) {
        console.error("Failed to run normalize_backlog via WASM, falling back to TS:", err);
    }
    // Fallback TS logic: identical to original
    const items = Array.isArray(backlogRaw)
        ? backlogRaw
        : backlogRaw?.items ?? backlogRaw?.results ?? [];
    return items
        .filter((it) => !appId || (it.app_id ?? it.appId ?? it.project) === appId || it.app_id == null)
        .slice(0, 8)
        .map((it) => ({
        id: String(it.id ?? it.key ?? it.title ?? ''),
        title: it.title ?? it.summary ?? it.name ?? String(it.id ?? 'item'),
        status: it.status ?? it.state ?? 'open'
    }));
}
/**
 * Normalizes the pack info.
 * Note: normalize_pack operation does not exist currently in maloca-core (Rust).
 * If it gets added, it will be called via WASM. Otherwise, we use TS fallback.
 */
export async function normalizePack(packRaw) {
    try {
        const client = await getWasmClient();
        if (client && typeof client.apply === "function") {
            // Check if normalize_pack op is available/supported by calling it in try/catch
            try {
                const result = await client.apply("normalize_pack", { json: packRaw });
                if (result) {
                    return {
                        features: result.features_total ?? 0,
                        decisions: result.decisions_count ?? 0
                    };
                }
            }
            catch (err) {
                // Op normalize_pack doesn't exist or failed, fallback to TS
            }
        }
    }
    catch (err) {
        // WASM client call failed
    }
    // Fallback TS logic
    if (!packRaw)
        return null;
    return {
        features: packRaw.features_total ?? 0,
        decisions: packRaw.decisions_count ?? 0
    };
}
