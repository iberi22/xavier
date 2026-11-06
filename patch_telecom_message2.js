const fs = require('fs');
const file = 'panel-ui/src/hooks/useTelecomRooms.ts';
let content = fs.readFileSync(file, 'utf8');

const search2 = `  // Optimistic Message Appending
  const sendMessageOptimistic = useCallback(
    (
      roomId: string,
      content: string,
      options?: {
        senderAlias?: string;
        senderNodeId?: string;
        attachment?: TelecomMessage["attachment"];
      }
    ): TelecomMessage => {`;

const replace2 = `  // Optimistic Message Appending
  const sendMessageOptimistic = useCallback(
    (
      roomId: string,
      content: string,
      options?: {
        senderAlias?: string;
        senderNodeId?: string;
        attachment?: TelecomMessage["attachment"];
        isEphemeral?: boolean;
      }
    ): TelecomMessage => {`;

if (content.includes(search2)) {
    content = content.replace(search2, replace2);
    fs.writeFileSync(file, content);
    console.log("Successfully added isEphemeral to sendMessageOptimistic signature.");
} else {
    console.error("Could not find insertion point 2.");
}

const search4 = `        encrypted: true,
        isSelf: true,
        pending: true,
        attachment: options?.attachment,
      };`;

const replace4 = `        encrypted: true,
        isSelf: true,
        pending: true,
        attachment: options?.attachment,
        isEphemeral: options?.isEphemeral,
      };`;

if (content.includes(search4)) {
    content = content.replace(search4, replace4);
    fs.writeFileSync(file, content);
    console.log("Successfully set isEphemeral in newMsg.");
} else {
    console.error("Could not find insertion point 4.");
}
