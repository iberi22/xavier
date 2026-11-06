const fs = require('fs');
const file = 'panel-ui/src/components/Telecom/TelecomHubView.tsx';
let content = fs.readFileSync(file, 'utf8');

const search = `      sendMessageOptimistic(selectedRoomId, inputMessage.trim(), { senderAlias: "Local Node (You)" });

      setInputMessage("");
      setIsSending(false);
    },
    [inputMessage, isSending, selectedRoomId, sendMessageOptimistic]`;

const replace = `      sendMessageOptimistic(selectedRoomId, inputMessage.trim(), { senderAlias: "Local Node (You)", isEphemeral });

      setInputMessage("");
      setIsSending(false);
    },
    [inputMessage, isSending, selectedRoomId, isEphemeral, sendMessageOptimistic]`;

if (content.includes(search)) {
    content = content.replace(search, replace);
    fs.writeFileSync(file, content);
    console.log("Successfully passed isEphemeral to sendMessageOptimistic.");
} else {
    console.error("Could not find insertion point.");
}
