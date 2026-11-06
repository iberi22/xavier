const fs = require('fs');
const file = 'panel-ui/tests/e2e/telecom-hub-rooms.spec.ts';
let content = fs.readFileSync(file, 'utf8');

const search = `    // Send an optimistic message in the room
    const messageInput = page.locator('input[placeholder*="Send encrypted message"]');
    await expect(messageInput).toBeVisible();
    await messageInput.fill("Telecom Hub Optimistic Dispatch Test");

    const sendBtn = page.locator('button[aria-label="Send Message"]');
    await expect(sendBtn).toBeEnabled();
    await sendBtn.click();

    // Assert optimistic message appeared in chat feed (scope to the message panel — the room
    // list item's lastMessage preview also renders this text, causing a strict-mode collision)
    const sentMsg = page
      .locator("section")
      .getByText("Telecom Hub Optimistic Dispatch Test");
    await expect(sentMsg).toBeVisible();`;

const replace = `    // Toggle Ephemeral Mode
    const ephemeralToggle = page.locator('button[aria-label="Toggle Ephemeral Mode"]');
    await expect(ephemeralToggle).toBeVisible();
    await ephemeralToggle.click();

    // Send an optimistic message in the room
    const messageInput = page.locator('input[placeholder*="Send encrypted message"]');
    await expect(messageInput).toBeVisible();
    await messageInput.fill("Telecom Hub Optimistic Dispatch Test");

    const sendBtn = page.locator('button[aria-label="Send Message"]');
    await expect(sendBtn).toBeEnabled();
    await sendBtn.click();

    // Assert optimistic message appeared in chat feed (scope to the message panel — the room
    // list item's lastMessage preview also renders this text, causing a strict-mode collision)
    const sentMsg = page
      .locator("section")
      .getByText("Telecom Hub Optimistic Dispatch Test");
    await expect(sentMsg).toBeVisible();

    // Verify Ephemeral badge is rendered with the message
    const ephemeralBadge = page.locator("section").getByText("Ephemeral").first();
    await expect(ephemeralBadge).toBeVisible();`;

if (content.includes(search)) {
    content = content.replace(search, replace);
    fs.writeFileSync(file, content);
    console.log("Successfully added ephemeral toggle and badge assertion to e2e test.");
} else {
    console.error("Could not find insertion point.");
}
