/**
 * WebAuthn PRF Extension Hardware Bridge for panel-ui.
 * Provides hardware-backed biometric deterministic seed derivation from client WebAuthn PRF extensions.
 */

export interface WebAuthnPrfEvalInput {
	first: Uint8Array;
	second?: Uint8Array;
}

export interface WebAuthnPrfExtensionInput {
	eval?: WebAuthnPrfEvalInput;
	evalByCredential?: Record<string, WebAuthnPrfEvalInput>;
}

export interface WebAuthnPrfExtensionOutput {
	enabled?: boolean;
	results?: {
		first: ArrayBuffer;
		second?: ArrayBuffer;
	};
}

export interface HardwareSeedResult {
	seed: Uint8Array;
	credentialId: string;
}

/**
 * Default 32-byte domain salt used for Xavier Node hardware seed derivation.
 */
export const DEFAULT_XAVIER_PRF_SALT = new Uint8Array([
	0x78, 0x61, 0x76, 0x69, 0x65, 0x72, 0x2d, 0x77, // "xavier-w"
	0x65, 0x62, 0x61, 0x75, 0x74, 0x68, 0x6e, 0x2d, // "ebauthn-"
	0x70, 0x72, 0x66, 0x2d, 0x73, 0x61, 0x6c, 0x74, // "prf-salt"
	0x2d, 0x76, 0x31, 0x2d, 0x73, 0x65, 0x65, 0x64, // "-v1-seed"
]);

/**
 * Checks whether WebAuthn and the PRF (Pseudo-Random Function) extension are supported by the client browser.
 */
export async function isWebAuthnPrfSupported(): Promise<boolean> {
	if (
		typeof window === "undefined" ||
		typeof navigator === "undefined" ||
		!navigator.credentials ||
		typeof window.PublicKeyCredential === "undefined"
	) {
		return false;
	}

	try {
		if (
			typeof window.PublicKeyCredential.getClientCapabilities === "function"
		) {
			const capabilities =
				await window.PublicKeyCredential.getClientCapabilities();
			return Boolean(capabilities && (capabilities as any).prf);
		}
		return true;
	} catch {
		return false;
	}
}

/**
 * Derives a 32-byte deterministic seed directly from the hardware enclave via WebAuthn PRF extension.
 * If a credentialId is provided, attempts navigator.credentials.get().
 * Otherwise, attempts navigator.credentials.get() or create() with PRF eval extension.
 * Returns null if WebAuthn PRF is unsupported, rejected, or unavailable.
 */
export async function deriveHardwareSeedWithPrf(
	credentialId?: string,
	saltBytes: Uint8Array = DEFAULT_XAVIER_PRF_SALT,
): Promise<HardwareSeedResult | null> {
	if (!(await isWebAuthnPrfSupported())) {
		return null;
	}

	const challenge = new Uint8Array(32);
	if (typeof window !== "undefined" && window.crypto?.getRandomValues) {
		window.crypto.getRandomValues(challenge);
	}

	try {
		// If a credential ID is specified, perform an assertion request (get)
		if (credentialId) {
			const allowCredentials: PublicKeyCredentialDescriptor[] = [
				{
					id: hexToBuf(credentialId),
					type: "public-key",
				},
			];

			const getOptions: CredentialRequestOptions = {
				publicKey: {
					challenge,
					timeout: 60000,
					userVerification: "preferred",
					allowCredentials,
					extensions: {
						prf: {
							eval: {
								first: saltBytes,
							},
						},
					} as any,
				},
			};

			const credential = (await navigator.credentials.get(
				getOptions,
			)) as PublicKeyCredential | null;

			if (credential) {
				const extResults = credential.getClientExtensionResults() as {
					prf?: WebAuthnPrfExtensionOutput;
				};

				if (extResults?.prf?.results?.first) {
					const seed = new Uint8Array(extResults.prf.results.first);
					return {
						seed,
						credentialId: credential.id || credentialId,
					};
				}
			}
		}

		// Try assertion (get) first without strict credential ID filter if supported
		try {
			const getOptions: CredentialRequestOptions = {
				publicKey: {
					challenge,
					timeout: 30000,
					userVerification: "preferred",
					extensions: {
						prf: {
							eval: {
								first: saltBytes,
							},
						},
					} as any,
				},
			};

			const credential = (await navigator.credentials.get(
				getOptions,
			)) as PublicKeyCredential | null;

			if (credential) {
				const extResults = credential.getClientExtensionResults() as {
					prf?: WebAuthnPrfExtensionOutput;
				};

				if (extResults?.prf?.results?.first) {
					const seed = new Uint8Array(extResults.prf.results.first);
					const credId =
						credential.id || bufToHex(new Uint8Array(credential.rawId));
					return {
						seed,
						credentialId: credId,
					};
				}
			}
		} catch {
			// Get assertion failed or no registered passkey; fall through to create
		}

		// Fallback to creation request (create) to register/derive hardware key seed
		const userId = new Uint8Array(16);
		if (typeof window !== "undefined" && window.crypto?.getRandomValues) {
			window.crypto.getRandomValues(userId);
		}

		const rpId =
			typeof window !== "undefined" && window.location?.hostname
				? window.location.hostname
				: "localhost";

		const createOptions: CredentialCreationOptions = {
			publicKey: {
				challenge,
				rp: {
					name: "Xavier Sovereign Mesh Node",
					id: rpId,
				},
				user: {
					id: userId,
					name: "xavier-node-identity",
					displayName: "Xavier Node Identity",
				},
				pubKeyCredParams: [
					{ alg: -7, type: "public-key" }, // ES256
					{ alg: -257, type: "public-key" }, // RS256
				],
				authenticatorSelection: {
					userVerification: "preferred",
					residentKey: "preferred",
				},
				timeout: 60000,
				extensions: {
					prf: {
						eval: {
							first: saltBytes,
						},
					},
				} as any,
			},
		};

		const newCredential = (await navigator.credentials.create(
			createOptions,
		)) as PublicKeyCredential | null;

		if (newCredential) {
			const extResults = newCredential.getClientExtensionResults() as {
				prf?: WebAuthnPrfExtensionOutput;
			};

			if (extResults?.prf?.results?.first) {
				const seed = new Uint8Array(extResults.prf.results.first);
				const credId =
					newCredential.id || bufToHex(new Uint8Array(newCredential.rawId));
				return {
					seed,
					credentialId: credId,
				};
			}
		}
	} catch (err) {
		console.warn("WebAuthn PRF seed derivation failed or was cancelled:", err);
	}

	return null;
}

function bufToHex(buf: Uint8Array): string {
	return Array.from(buf)
		.map((b) => b.toString(16).padStart(2, "0"))
		.join("");
}

function hexToBuf(hex: string): Uint8Array {
	const bytes = new Uint8Array(Math.ceil(hex.length / 2));
	for (let i = 0; i < bytes.length; i++) {
		bytes[i] = parseInt(hex.substring(i * 2, i * 2 + 2), 16) || 0;
	}
	return bytes;
}
