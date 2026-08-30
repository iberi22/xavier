// singleton for maloca-wasm with TS fallback

let malocaWasm: any = null;
let isInitialized = false;

// Initialize the WASM module asynchronously in the background.
// This is executed once as a background side-effect on module load.
export async function tryInitWasm(): Promise<void> {
  try {
    // @ts-ignore
    const mod = await import("@swal/maloca-wasm/main");
    if (mod) {
      if (typeof mod.initMalocaWasm === "function") {
        await mod.initMalocaWasm();
      }
      malocaWasm = mod.malocaWasm;
      isInitialized = true;
    }
  } catch (err) {
    // Graceful degradation (fallback TS local parsing if WASM fails/is missing)
    console.warn("[wasmBridge] maloca-wasm is not available, using TS fallback", err);
  }
}

// Trigger background initialization
tryInitWasm().catch((err) => {
  console.warn("[wasmBridge] Silent background initialization failed:", err);
});

/**
 * Executes a WASM operation synchronously if available, otherwise executes the fallback function.
 * This ensures progressive enhancement and graceful degradation of the application without
 * introducing asynchronous delays/race conditions during high-frequency events.
 *
 * @param op The operation name to execute (e.g., "next_backoff_ms", "classify_frame")
 * @param payload The arguments/payload for the operation
 * @param fallback The fallback function to run if WASM is not loaded or fails
 */
export function wasmApply<T>(
  op: string,
  payload: any,
  fallback: () => T
): T {
  if (isInitialized && malocaWasm && typeof malocaWasm.apply === "function") {
    try {
      const res = malocaWasm.apply(op, payload);
      if (res !== undefined && res !== null) {
        return res as T;
      }
    } catch (err) {
      // Graceful fallback (degradación) to TS
      console.warn(`Failed to execute WASM operation "${op}" synchronously, falling back to TS:`, err);
    }
  }
  return fallback();
}
