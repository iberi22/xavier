import sys
content = open('panel-ui/tests/e2e/settings-security-light-theme.spec.ts').read()
if "await page.waitForLoadState('domcontentloaded');" not in content:
    content = content.replace('''
    // Open Settings Modal via BrainCircuit button
    const brainButton = page.locator('button[aria-label="Open Control Node"]');
    await expect(brainButton).toBeVisible();''', '''
    await page.waitForLoadState('domcontentloaded');
    await page.waitForTimeout(1000);

    // Open Settings Modal via BrainCircuit button
    const brainButton = page.locator('button[aria-label="Open Control Node"]');
    await expect(brainButton).toBeVisible({ timeout: 15000 });''')
    with open('panel-ui/tests/e2e/settings-security-light-theme.spec.ts', 'w') as f:
        f.write(content)
