import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { useAuthStore } from "./AuthProvider";
import { parseLoginHashQuery } from "./LoginPage";

const RT_KEY = "xavier.rt";
const okBody = {
	user: { id: "u1", email: "a@b.c" },
	access_token: "jwt",
	refresh_token: "rt1",
	requires_2fa: false,
};

const reset = () =>
	useAuthStore.setState({
		user: null,
		token: null,
		accessToken: null,
		refreshToken: null,
		isAuthenticated: false,
	});

describe("parseLoginHashQuery", () => {
	it("extracts email and code", () => {
		expect(parseLoginHashQuery("#/login?email=a%40b.c&code=123456")).toEqual({
			email: "a@b.c",
			code: "123456",
		});
	});
	it("returns null when missing or malformed", () => {
		expect(parseLoginHashQuery("#/login")).toBeNull();
		expect(parseLoginHashQuery("#/login?email=a@b.c")).toBeNull();
		expect(parseLoginHashQuery("#/login?email=a@b.c&code=12ab56")).toBeNull();
		expect(parseLoginHashQuery("")).toBeNull();
	});
});

describe("auth store with login codes", () => {
	beforeEach(() => {
		sessionStorage.clear();
		reset();
	});
	afterEach(() => vi.unstubAllGlobals());

	it("loginWithCode success stores session and persists only the refresh token", async () => {
		const f = vi.fn().mockResolvedValue(new Response(JSON.stringify(okBody)));
		vi.stubGlobal("fetch", f);
		await useAuthStore.getState().loginWithCode("a@b.c", "123456");
		expect(String(f.mock.calls[0][0])).toContain("/auth/exchange");
		expect(JSON.parse(f.mock.calls[0][1].body)).toEqual({
			email: "a@b.c",
			code: "123456",
		});
		expect(useAuthStore.getState().isAuthenticated).toBe(true);
		expect(sessionStorage.getItem(RT_KEY)).toBe("rt1");
		expect(JSON.stringify({ ...sessionStorage })).not.toContain("jwt");
	});

	it("loginWithCode 401 throws invalid_code and stores nothing", async () => {
		vi.stubGlobal(
			"fetch",
			vi
				.fn()
				.mockResolvedValue(
					new Response('{"error":"invalid_code"}', { status: 401 }),
				),
		);
		await expect(
			useAuthStore.getState().loginWithCode("a@b.c", "000000"),
		).rejects.toMatchObject({ code: "invalid_code" });
		expect(useAuthStore.getState().isAuthenticated).toBe(false);
		expect(sessionStorage.getItem(RT_KEY)).toBeNull();
	});

	it("refreshSession reads the stored token, rotates it, and clears on failure", async () => {
		sessionStorage.setItem(RT_KEY, "old");
		const f = vi
			.fn()
			.mockResolvedValueOnce(
				new Response(
					JSON.stringify({ access_token: "j2", refresh_token: "new" }),
				),
			)
			.mockResolvedValueOnce(new Response("", { status: 401 }));
		vi.stubGlobal("fetch", f);
		await useAuthStore.getState().refreshSession();
		expect(JSON.parse(f.mock.calls[0][1].body).refresh_token).toBe("old");
		expect(sessionStorage.getItem(RT_KEY)).toBe("new");
		await useAuthStore.getState().refreshSession();
		expect(sessionStorage.getItem(RT_KEY)).toBeNull();
		expect(useAuthStore.getState().isAuthenticated).toBe(false);
	});

	it("logout removes the stored refresh token", async () => {
		sessionStorage.setItem(RT_KEY, "rt");
		vi.stubGlobal("fetch", vi.fn().mockResolvedValue(new Response("")));
		await useAuthStore.getState().logout();
		expect(sessionStorage.getItem(RT_KEY)).toBeNull();
	});
});
