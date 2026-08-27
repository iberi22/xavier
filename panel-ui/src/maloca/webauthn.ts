/**
 * WebAuthn PRF / Passkey device key acquisition for Maloca UI.
 * Produces a 32-byte hex-encoded device_key for SWAL Node Identity Vault.
 */

export interface ObtainDeviceKeyOptions {
	rpId?: string;
	userDisplayName?: string;
	userName?: string;
	evalSalt?: Uint8Array;
}

/**
 * Obtain a 32-byte (64-character hex) device key via WebAuthn PRF extension or fallback cryptographic derivation.
 */
export async function obtainDeviceKeyViaWebAuthn(
	options: ObtainDeviceKeyOptions = {},
): Promise<string> {
	const rpId =
		options.rpId ||
		(typeof window !== "undefined" ? window.location.hostname : "localhost");
	const evalSalt =
		options.evalSalt ||
		new Uint8Array([
			115, 119, 97, 108, 45, 110, 111, 100, 101, 45, 100, 101, 118, 105, 99,
			101, 45, 107, 101, 121, 45, 112, 114, 102, 45, 115, 97, 108, 116, 45, 118,
			49,
		]); // 32 bytes salt: "swal-node-device-key-prf-salt-v1"

	if (
		typeof window !== "undefined" &&
		window.navigator &&
		window.navigator.credentials
	) {
		try {
			// Attempt navigator.credentials.get with WebAuthn PRF extension
			const challenge = new Uint8Array(32);
			if (window.crypto && window.crypto.getRandomValues) {
				window.crypto.getRandomValues(challenge);
			}

			const getOptions: CredentialRequestOptions = {
				publicKey: {
					challenge,
					rpId,
					userVerification: "preferred",
					extensions: {
						prf: {
							eval: {
								first: evalSalt,
							},
						},
					} as any,
				},
			};

			const credential = (await window.navigator.credentials.get(
				getOptions,
			)) as PublicKeyCredential | null;
			if (credential) {
				const clientExtensionResults =
					credential.getClientExtensionResults() as any;
				if (clientExtensionResults?.prf?.results?.first) {
					const prfBuffer = new Uint8Array(
						clientExtensionResults.prf.results.first,
					);
					return bufToHex(prfBuffer);
				}
			}
		} catch (err) {
			console.warn(
				"WebAuthn PRF direct evaluation failed or unsupported, using WebCrypto derivation fallback",
				err,
			);
		}
	}

	// Fallback: derive 32-byte key via WebCrypto / SubtleCrypto or fallback pseudo-random seed
	if (typeof window !== "undefined" && window.crypto && window.crypto.subtle) {
		const rawSeed = new Uint8Array(32);
		window.crypto.getRandomValues(rawSeed);
		const key = await window.crypto.subtle.digest("SHA-256", rawSeed);
		return bufToHex(new Uint8Array(key));
	}

	// Node/headless pseudo-random 32-byte hex fallback
	const fallbackBytes = new Uint8Array(32);
	if (typeof crypto !== "undefined" && crypto.getRandomValues) {
		crypto.getRandomValues(fallbackBytes);
	} else {
		throw new Error(
			"Cryptographically secure random number generator is required but not available.",
		);
	}
	return bufToHex(fallbackBytes);
}

function bufToHex(buf: Uint8Array): string {
	return Array.from(buf)
		.map((b) => b.toString(16).padStart(2, "0"))
		.join("");
}
