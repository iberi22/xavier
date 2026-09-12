import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
	executeNodeLogin,
	getOrCreateNodeIdentity,
	startHeartbeatDaemon,
} from "./nodeIdentity";
import {
	DEFAULT_XAVIER_PRF_SALT,
	deriveHardwareSeedWithPrf,
	isWebAuthnPrfSupported,
} from "./webauthnPrf";

describe("WebAuthn PRF extension hardware bridge", () => {
	const originalNavigator = global.navigator;
	const originalPublicKeyCredential = global.PublicKeyCredential;

	beforeEach(() => {
		vi.resetAllMocks();
		vi.restoreAllMocks();
	});

	afterEach(() => {
		Object.defineProperty(global, "navigator", {
			value: originalNavigator,
			writable: true,
			configurable: true,
		});
		Object.defineProperty(global, "PublicKeyCredential", {
			value: originalPublicKeyCredential,
			writable: true,
			configurable: true,
		});
	});

	describe("isWebAuthnPrfSupported", () => {
		it("returns false when navigator.credentials is undefined", async () => {
			Object.defineProperty(global, "navigator", {
				value: {},
				writable: true,
				configurable: true,
			});
			const supported = await isWebAuthnPrfSupported();
			expect(supported).toBe(false);
		});

		it("returns true when PublicKeyCredential client capabilities reports prf: true", async () => {
			const mockGetCapabilities = vi.fn().mockResolvedValue({ prf: true });
			const mockPublicKeyCredential = function () {} as any;
			mockPublicKeyCredential.getClientCapabilities = mockGetCapabilities;

			Object.defineProperty(global, "PublicKeyCredential", {
				value: mockPublicKeyCredential,
				writable: true,
				configurable: true,
			});

			Object.defineProperty(global, "navigator", {
				value: {
					credentials: {
						get: vi.fn(),
						create: vi.fn(),
					},
				},
				writable: true,
				configurable: true,
			});

			const supported = await isWebAuthnPrfSupported();
			expect(supported).toBe(true);
			expect(mockGetCapabilities).toHaveBeenCalled();
		});

		it("returns false when PublicKeyCredential client capabilities reports prf: false", async () => {
			const mockGetCapabilities = vi.fn().mockResolvedValue({ prf: false });
			const mockPublicKeyCredential = function () {} as any;
			mockPublicKeyCredential.getClientCapabilities = mockGetCapabilities;

			Object.defineProperty(global, "PublicKeyCredential", {
				value: mockPublicKeyCredential,
				writable: true,
				configurable: true,
			});

			Object.defineProperty(global, "navigator", {
				value: {
					credentials: {
						get: vi.fn(),
						create: vi.fn(),
					},
				},
				writable: true,
				configurable: true,
			});

			const supported = await isWebAuthnPrfSupported();
			expect(supported).toBe(false);
		});

		it("returns true when PublicKeyCredential exists without getClientCapabilities", async () => {
			const mockPublicKeyCredential = function () {} as any;

			Object.defineProperty(global, "PublicKeyCredential", {
				value: mockPublicKeyCredential,
				writable: true,
				configurable: true,
			});

			Object.defineProperty(global, "navigator", {
				value: {
					credentials: {
						get: vi.fn(),
						create: vi.fn(),
					},
				},
				writable: true,
				configurable: true,
			});

			const supported = await isWebAuthnPrfSupported();
			expect(supported).toBe(true);
		});
	});

	describe("deriveHardwareSeedWithPrf", () => {
		it("returns deterministic seed and credentialId when navigator.credentials.get returns PRF output", async () => {
			const mockSeed = new Uint8Array(32).fill(42);
			const mockCredentialId = "cred_test_123";

			const mockGet = vi.fn().mockResolvedValue({
				id: mockCredentialId,
				rawId: new Uint8Array([1, 2, 3]).buffer,
				getClientExtensionResults: () => ({
					prf: {
						results: {
							first: mockSeed.buffer,
						},
					},
				}),
			});

			Object.defineProperty(global, "PublicKeyCredential", {
				value: function () {} as any,
				writable: true,
				configurable: true,
			});

			Object.defineProperty(global, "navigator", {
				value: {
					credentials: {
						get: mockGet,
						create: vi.fn(),
					},
				},
				writable: true,
				configurable: true,
			});

			const result = await deriveHardwareSeedWithPrf(mockCredentialId);
			expect(result).not.toBeNull();
			expect(result?.credentialId).toBe(mockCredentialId);
			expect(result?.seed).toEqual(mockSeed);
		});

		it("returns seed and credentialId from navigator.credentials.create when get fails", async () => {
			const mockSeed = new Uint8Array(32).fill(99);
			const mockCreatedCredId = "cred_created_456";

			const mockGet = vi.fn().mockRejectedValue(new Error("No credentials"));
			const mockCreate = vi.fn().mockResolvedValue({
				id: mockCreatedCredId,
				rawId: new Uint8Array([4, 5, 6]).buffer,
				getClientExtensionResults: () => ({
					prf: {
						results: {
							first: mockSeed.buffer,
						},
					},
				}),
			});

			Object.defineProperty(global, "PublicKeyCredential", {
				value: function () {} as any,
				writable: true,
				configurable: true,
			});

			Object.defineProperty(global, "navigator", {
				value: {
					credentials: {
						get: mockGet,
						create: mockCreate,
					},
				},
				writable: true,
				configurable: true,
			});

			const result = await deriveHardwareSeedWithPrf();
			expect(result).not.toBeNull();
			expect(result?.credentialId).toBe(mockCreatedCredId);
			expect(result?.seed).toEqual(mockSeed);
		});

		it("returns null when WebAuthn calls reject or return no PRF results", async () => {
			const mockGet = vi.fn().mockRejectedValue(new Error("User cancelled"));
			const mockCreate =
				vi.fn().mockRejectedValue(new Error("Not allowed error"));

			Object.defineProperty(global, "PublicKeyCredential", {
				value: function () {} as any,
				writable: true,
				configurable: true,
			});

			Object.defineProperty(global, "navigator", {
				value: {
					credentials: {
						get: mockGet,
						create: mockCreate,
					},
				},
				writable: true,
				configurable: true,
			});

			const result = await deriveHardwareSeedWithPrf();
			expect(result).toBeNull();
		});
	});

	describe("nodeIdentity integration", () => {
		it("creates standard software identity when useHardwareEnclave is false or omitted", async () => {
			const identity = await getOrCreateNodeIdentity();
			expect(identity.hardwareBacked).toBe(false);
			expect(identity.nodeId).toMatch(/^node_/);
			expect(identity.publicKeyHex).toBeDefined();
		});

		it("creates hardwareBacked node identity with deterministic keypair matching and signing verification", async () => {
			const mockSeed = new Uint8Array(32).fill(77);
			const mockCredId = "cred_hw_77";

			const mockGet = vi.fn().mockResolvedValue({
				id: mockCredId,
				rawId: new Uint8Array([7, 7, 7]).buffer,
				getClientExtensionResults: () => ({
					prf: {
						results: {
							first: mockSeed.buffer,
						},
					},
				}),
			});

			Object.defineProperty(global, "PublicKeyCredential", {
				value: function () {} as any,
				writable: true,
				configurable: true,
			});

			Object.defineProperty(global, "navigator", {
				value: {
					credentials: {
						get: mockGet,
						create: vi.fn(),
					},
				},
				writable: true,
				configurable: true,
			});

			const identity1 = await getOrCreateNodeIdentity({
				useHardwareEnclave: true,
			});
			const identity2 = await getOrCreateNodeIdentity({
				useHardwareEnclave: true,
			});

			expect(identity1.hardwareBacked).toBe(true);
			expect(identity1.credentialId).toBe(mockCredId);
			expect(identity1.nodeId).toMatch(/^node_/);
			// Determinism check: same seed results in same nodeId and publicKeyHex
			expect(identity1.nodeId).toBe(identity2.nodeId);
			expect(identity1.publicKeyHex).toBe(identity2.publicKeyHex);

			// Test signature verification with the matched public key
			const loginResult = await executeNodeLogin(identity1, { test: "data" });
			expect(loginResult.success).toBe(true);
		});

		it("falls back gracefully to software identity when useHardwareEnclave is true but PRF fails", async () => {
			Object.defineProperty(global, "PublicKeyCredential", {
				value: function () {} as any,
				writable: true,
				configurable: true,
			});

			Object.defineProperty(global, "navigator", {
				value: {
					credentials: {
						get: vi.fn().mockRejectedValue(new Error("Hardware key missing")),
						create: vi.fn().mockRejectedValue(new Error("Denied")),
					},
				},
				writable: true,
				configurable: true,
			});

			const identity = await getOrCreateNodeIdentity({
				useHardwareEnclave: true,
			});
			expect(identity.hardwareBacked).toBe(false);
			expect(identity.nodeId).toMatch(/^node_/);
		});

		it("executes node login and returns signed payload response", async () => {
			const identity = await getOrCreateNodeIdentity();
			const loginResult = await executeNodeLogin(identity, {
				session: "test_session",
			});

			expect(loginResult.success).toBe(true);
			expect(loginResult.nodeId).toBe(identity.nodeId);
			expect(loginResult.timestamp).toBeGreaterThan(0);
			expect(typeof loginResult.signature).toBe("string");
		});

		it("starts and stops heartbeat daemon without throwing", async () => {
			const identity = await getOrCreateNodeIdentity();
			const controller = startHeartbeatDaemon(identity, 100);

			expect(controller).toBeDefined();
			expect(typeof controller.stop).toBe("function");
			controller.stop();
		});
	});
});
