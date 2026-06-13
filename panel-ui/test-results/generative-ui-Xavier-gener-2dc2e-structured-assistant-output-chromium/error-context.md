# Instructions

- Following Playwright test failed.
- Explain why, be concise, respect Playwright best practices.
- Provide a snippet of code with the fix, if possible.

# Test info

- Name: generative-ui.spec.ts >> Xavier generative panel >> creates threads, preserves empty state, and renders structured assistant output
- Location: tests\generative-ui.spec.ts:58:3

# Error details

```
Error: expect(locator).toBeVisible() failed

Locator: getByText('OpenUI cockpit for the internal agent')
Expected: visible
Timeout: 15000ms
Error: element(s) not found

Call log:
  - Expect "toBeVisible" with timeout 15000ms
  - waiting for getByText('OpenUI cockpit for the internal agent')

```

```yaml
- heading "XAVIER AUTH" [level=1]
- paragraph: Enter your master terminal token to connect to the local code graph vector system.
- textbox "XAVIER_TOKEN"
- button "INITIALIZE SESSION"
```

# Test source

```ts
  1   | import { expect, type Page, test } from "@playwright/test";
  2   | 
  3   | const appPath = process.env.PANEL_UI_APP_PATH ?? "/";
  4   | const panelApiRoot = "/panel/api";
  5   | const assetRoot =
  6   |   appPath === "/" ? "/assets" : `${appPath.replace(/\/$/, "")}/assets`;
  7   | const testToken = process.env.XAVIER_TOKEN || "dev-token";
  8   | const prompts = [
  9   |   "Explain xavier memory and show the answer as a structured UI.",
  10  |   "Summarize the current agent workflow as cards with supporting details.",
  11  | ];
  12  | 
  13  | async function enterPanel(page: Page) {
  14  |   await page.goto(appPath);
  15  | 
  16  |   await expect(
  17  |     page.getByText("OpenUI cockpit for the internal agent"),
> 18  |   ).toBeVisible();
      |     ^ Error: expect(locator).toBeVisible() failed
  19  | 
  20  |   await page.getByPlaceholder("XAVIER_TOKEN").fill(testToken);
  21  |   await page.getByRole("button", { name: "Enter panel" }).click();
  22  | 
  23  |   await expect(page.getByRole("button", { name: "New thread" })).toBeVisible();
  24  | }
  25  | 
  26  | test.describe("Xavier generative panel", () => {
  27  |   test("keeps the shell public while protecting panel APIs and assets", async ({
  28  |     page,
  29  |     request,
  30  |   }) => {
  31  |     const shellResponse = await request.get(appPath);
  32  |     expect(shellResponse.status()).toBe(200);
  33  | 
  34  |     const assetResponse = await request.get(`${assetRoot}/index.js`);
  35  |     expect(assetResponse.status()).toBe(200);
  36  |     expect(assetResponse.headers()["content-type"]).toContain("javascript");
  37  | 
  38  |     const missingAssetResponse = await request.get(`${assetRoot}/missing.js`);
  39  |     expect(missingAssetResponse.status()).toBe(404);
  40  | 
  41  |     const unauthorizedThreadsResponse = await request.get(
  42  |       `${panelApiRoot}/threads`,
  43  |     );
  44  |     expect(unauthorizedThreadsResponse.status()).toBe(401);
  45  | 
  46  |     const authorizedThreadsResponse = await request.get(
  47  |       `${panelApiRoot}/threads`,
  48  |       {
  49  |         headers: { "X-Xavier-Token": testToken },
  50  |       },
  51  |     );
  52  |     expect(authorizedThreadsResponse.status()).toBe(200);
  53  |     expect(await authorizedThreadsResponse.json()).toEqual(expect.any(Array));
  54  | 
  55  |     await enterPanel(page);
  56  |   });
  57  | 
  58  |   test("creates threads, preserves empty state, and renders structured assistant output", async ({
  59  |     page,
  60  |     request,
  61  |   }) => {
  62  |     await enterPanel(page);
  63  | 
  64  |     await page.getByRole("button", { name: "New thread" }).click();
  65  |     await expect(page.locator(".topbar h1")).toHaveText("New Thread");
  66  |     await expect(page.locator(".message-card")).toHaveCount(0);
  67  | 
  68  |     const composer = page.getByPlaceholder(
  69  |       "Ask Xavier for memory, code, or a structured answer...",
  70  |     );
  71  | 
  72  |     for (const prompt of prompts) {
  73  |       await composer.fill(prompt);
  74  |       await page.getByRole("button", { name: "Send" }).click();
  75  | 
  76  |       await expect(page.locator(".loading-block")).toBeVisible();
  77  |       await expect(page.locator(".loading-block")).toBeHidden({
  78  |         timeout: 45_000,
  79  |       });
  80  | 
  81  |       const renderSurface = page.locator(".render-surface").last();
  82  |       await expect(renderSurface).toBeVisible();
  83  |       await expect(
  84  |         renderSurface.getByText("OpenUI Render Surface"),
  85  |       ).toBeVisible();
  86  | 
  87  |       const assistantCards = page.locator(".assistant-card");
  88  |       await expect
  89  |         .poll(async () => assistantCards.count(), { timeout: 15_000 })
  90  |         .toBeGreaterThan(0);
  91  |       await expect(assistantCards.last().locator(".plain-text")).not.toHaveText(
  92  |         /^$/,
  93  |       );
  94  |     }
  95  | 
  96  |     await expect(page.locator(".topbar h1")).not.toHaveText("New Thread");
  97  | 
  98  |     const threadsResponse = await request.get(`${panelApiRoot}/threads`, {
  99  |       headers: { "X-Xavier-Token": testToken },
  100 |     });
  101 |     const threads = (await threadsResponse.json()) as Array<{
  102 |       id: string;
  103 |       title: string;
  104 |     }>;
  105 |     const activeThread = threads[0];
  106 | 
  107 |     expect(activeThread).toBeDefined();
  108 | 
  109 |     const detailResponse = await request.get(
  110 |       `${panelApiRoot}/threads/${activeThread?.id}`,
  111 |       {
  112 |         headers: { "X-Xavier-Token": testToken },
  113 |       },
  114 |     );
  115 |     expect(detailResponse.status()).toBe(200);
  116 | 
  117 |     const detail = (await detailResponse.json()) as {
  118 |       thread: { title: string };
```