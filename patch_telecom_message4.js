const fs = require('fs');
const file = 'panel-ui/src/hooks/useTelecomRooms.ts';
let content = fs.readFileSync(file, 'utf8');

const search4 = `        encrypted: true,
        isSelf: true,
        pending: true,
        failed: false,
        attachment: options?.attachment,
      };`;

const replace4 = `        encrypted: true,
        isSelf: true,
        pending: true,
        failed: false,
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
