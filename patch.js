const fs = require('fs');
const filepath = 'panel-ui/src/components/Mesh/MeshChatView.tsx';
let content = fs.readFileSync(filepath, 'utf8');

// Wrap handleSelectChannel in useCallback
content = content.replace(
  /const handleSelectChannel = \(channelId: string\) => \{([\s\S]*?)  \};/m,
  'const handleSelectChannel = React.useCallback((channelId: string) => {$1  }, []);'
);

// Replace networkRooms.map mapping
const networkRoomsMapStr = `              {networkRooms.map((room) => {
                const isActive = room.id === activeChannelId;
                return (
                  <button
                    key={room.id}
                    type="button"
                    onClick={() => handleSelectChannel(room.id)}
                    className={\`w-full flex items-center justify-between px-3 py-2 rounded-lg text-xs font-medium transition-colors \${
                      isActive
                        ? "bg-emerald-500/10 text-emerald-300 border border-emerald-500/20"
                        : "text-zinc-400 hover:text-zinc-200 hover:bg-zinc-900/50"
                    }\`}
                  >
                    <div className="flex items-center gap-2 truncate">
                      <Hash className={\`w-3.5 h-3.5 \${isActive ? "text-emerald-400" : "text-zinc-500"}\`} />
                      <span className="truncate">{room.name}</span>
                    </div>
                    {room.unreadCount && room.unreadCount > 0 ? (
                      <span className="bg-emerald-500 text-zinc-950 text-[10px] font-bold px-1.5 py-0.2 rounded-full font-mono">
                        {room.unreadCount}
                      </span>
                    ) : null}
                  </button>
                );
              })}`;

const newNetworkRoomsMapStr = `              {networkRooms.map((room) => (
                <RoomChannelItem
                  key={room.id}
                  room={room}
                  isActive={room.id === activeChannelId}
                  onSelect={handleSelectChannel}
                />
              ))}`;
content = content.replace(networkRoomsMapStr, newNetworkRoomsMapStr);

// Replace directPeers.map mapping
const directPeersMapStr = `              {directPeers.map((peer) => {
                const isActive = peer.id === activeChannelId;
                return (
                  <button
                    key={peer.id}
                    type="button"
                    onClick={() => handleSelectChannel(peer.id)}
                    className={\`w-full flex items-center justify-between px-3 py-2 rounded-lg text-xs font-medium transition-colors \${
                      isActive
                        ? "bg-emerald-500/10 text-emerald-300 border border-emerald-500/20"
                        : "text-zinc-400 hover:text-zinc-200 hover:bg-zinc-900/50"
                    }\`}
                  >
                    <div className="flex items-center gap-2 truncate">
                      <div className="relative flex-shrink-0">
                        <User className={\`w-3.5 h-3.5 \${isActive ? "text-emerald-400" : "text-zinc-500"}\`} />
                        <span
                          className={\`absolute -bottom-0.5 -right-0.5 w-2 h-2 rounded-full border border-[#0c0c0e] \${
                            peer.online ? "bg-emerald-400" : "bg-zinc-600"
                          }\`}
                        />
                      </div>
                      <div className="truncate text-left">
                        <span className="block truncate">{peer.name}</span>
                        {peer.nodeId && (
                          <span className="block text-[9px] text-zinc-600 font-mono truncate">
                            {peer.nodeId}
                          </span>
                        )}
                      </div>
                    </div>
                    {peer.unreadCount && peer.unreadCount > 0 ? (
                      <span className="bg-emerald-500 text-zinc-950 text-[10px] font-bold px-1.5 py-0.2 rounded-full font-mono">
                        {peer.unreadCount}
                      </span>
                    ) : null}
                  </button>
                );
              })}`;

const newDirectPeersMapStr = `              {directPeers.map((peer) => (
                <DirectPeerItem
                  key={peer.id}
                  peer={peer}
                  isActive={peer.id === activeChannelId}
                  onSelect={handleSelectChannel}
                />
              ))}`;
content = content.replace(directPeersMapStr, newDirectPeersMapStr);

const appendContent = `
/**
 * ⚡ Bolt Performance Optimization
 *
 * 💡 What: Extracted room channel row into RoomChannelItem and wrapped in React.memo()
 * 🎯 Why: Active channel switching triggered O(N) re-renders for every channel item.
 * 📊 Impact: Eliminates unnecessary re-renders of channel list items when switching channels.
 */
const RoomChannelItem = React.memo(function RoomChannelItem({
  room,
  isActive,
  onSelect,
}: {
  room: ChatChannel;
  isActive: boolean;
  onSelect: (id: string) => void;
}) {
  return (
    <button
      type="button"
      onClick={() => onSelect(room.id)}
      className={\`w-full flex items-center justify-between px-3 py-2 rounded-lg text-xs font-medium transition-colors \${
        isActive
          ? "bg-emerald-500/10 text-emerald-300 border border-emerald-500/20"
          : "text-zinc-400 hover:text-zinc-200 hover:bg-zinc-900/50"
      }\`}
    >
      <div className="flex items-center gap-2 truncate">
        <Hash className={\`w-3.5 h-3.5 \${isActive ? "text-emerald-400" : "text-zinc-500"}\`} />
        <span className="truncate">{room.name}</span>
      </div>
      {room.unreadCount && room.unreadCount > 0 ? (
        <span className="bg-emerald-500 text-zinc-950 text-[10px] font-bold px-1.5 py-0.2 rounded-full font-mono">
          {room.unreadCount}
        </span>
      ) : null}
    </button>
  );
});

/**
 * ⚡ Bolt Performance Optimization
 *
 * 💡 What: Extracted direct peer row into DirectPeerItem and wrapped in React.memo()
 * 🎯 Why: Active channel switching triggered O(N) re-renders for every peer item.
 * 📊 Impact: Eliminates unnecessary re-renders of peer list items when switching channels.
 */
const DirectPeerItem = React.memo(function DirectPeerItem({
  peer,
  isActive,
  onSelect,
}: {
  peer: ChatChannel;
  isActive: boolean;
  onSelect: (id: string) => void;
}) {
  return (
    <button
      type="button"
      onClick={() => onSelect(peer.id)}
      className={\`w-full flex items-center justify-between px-3 py-2 rounded-lg text-xs font-medium transition-colors \${
        isActive
          ? "bg-emerald-500/10 text-emerald-300 border border-emerald-500/20"
          : "text-zinc-400 hover:text-zinc-200 hover:bg-zinc-900/50"
      }\`}
    >
      <div className="flex items-center gap-2 truncate">
        <div className="relative flex-shrink-0">
          <User className={\`w-3.5 h-3.5 \${isActive ? "text-emerald-400" : "text-zinc-500"}\`} />
          <span
            className={\`absolute -bottom-0.5 -right-0.5 w-2 h-2 rounded-full border border-[#0c0c0e] \${
              peer.online ? "bg-emerald-400" : "bg-zinc-600"
            }\`}
          />
        </div>
        <div className="truncate text-left">
          <span className="block truncate">{peer.name}</span>
          {peer.nodeId && (
            <span className="block text-[9px] text-zinc-600 font-mono truncate">
              {peer.nodeId}
            </span>
          )}
        </div>
      </div>
      {peer.unreadCount && peer.unreadCount > 0 ? (
        <span className="bg-emerald-500 text-zinc-950 text-[10px] font-bold px-1.5 py-0.2 rounded-full font-mono">
          {peer.unreadCount}
        </span>
      ) : null}
    </button>
  );
});
`;

fs.writeFileSync(filepath, content + appendContent);
