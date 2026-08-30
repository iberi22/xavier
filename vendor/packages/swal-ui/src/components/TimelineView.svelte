<script lang="ts">
  import { onMount } from "svelte";
  import { fetchTimeline, fetchTimelineSessions, type TimelineEvent, type TimelineSession, fetchEventContext } from "../lib/timelineClient";

  let sessions = $state<TimelineSession[]>([]);
  let events = $state<TimelineEvent[]>([]);
  let expandedSessions = $state<Set<string>>(new Set());
  let expandedEvents = $state<Record<string, any>>({});
  
  let filterAgent = $state("");
  let filterAppId = $state("");
  let filterType = $state("");
  
  onMount(async () => {
    sessions = await fetchTimelineSessions();
    events = await fetchTimeline();
    
    // Auto-expand all sessions by default for visibility
    const sessionIds = new Set(sessions.map(s => s.id));
    expandedSessions = sessionIds;
  });

  const toggleSession = (id: string) => {
    const next = new Set(expandedSessions);
    if (next.has(id)) {
      next.delete(id);
    } else {
      next.add(id);
    }
    expandedSessions = next;
  };

  const toggleEventContext = async (id: string) => {
    if (expandedEvents[id]) {
      const next = { ...expandedEvents };
      delete next[id];
      expandedEvents = next;
    } else {
      const context = await fetchEventContext(id);
      expandedEvents = { ...expandedEvents, [id]: context || { message: "No additional context found" } };
    }
  };

  const getTypeColor = (type: string) => {
    switch (type) {
      case 'decision': return 'text-amber-500 bg-amber-500/10 border-amber-500/20';
      case 'memory_created': return 'text-cyan-500 bg-cyan-500/10 border-cyan-500/20';
      case 'commit': return 'text-green-500 bg-green-500/10 border-green-500/20';
      case 'error': return 'text-red-500 bg-red-500/10 border-red-500/20';
      default: return 'text-zinc-500 bg-zinc-500/10 border-zinc-500/20';
    }
  };

  const filteredEvents = $derived(events.filter(e => {
    if (filterAgent && !e.agent.toLowerCase().includes(filterAgent.toLowerCase())) return false;
    if (filterAppId && e.appId && !e.appId.toLowerCase().includes(filterAppId.toLowerCase())) return false;
    if (filterType && e.type !== filterType) return false;
    return true;
  }));

  const eventsBySession = $derived(filteredEvents.reduce((acc, ev) => {
    if (!acc[ev.sessionId]) acc[ev.sessionId] = [];
    acc[ev.sessionId].push(ev);
    return acc;
  }, {} as Record<string, TimelineEvent[]>));

</script>

<div class="flex flex-col gap-4">
  <div class="flex gap-4 mb-4 flex-wrap bg-zinc-900/50 p-4 rounded-lg border border-zinc-800">
    <input type="text" placeholder="Filter by agent..." bind:value={filterAgent} class="bg-zinc-950 border border-zinc-800 rounded px-3 py-1.5 text-sm text-zinc-200 focus:outline-none focus:border-emerald-500" />
    <input type="text" placeholder="Filter by app ID..." bind:value={filterAppId} class="bg-zinc-950 border border-zinc-800 rounded px-3 py-1.5 text-sm text-zinc-200 focus:outline-none focus:border-emerald-500" />
    <select bind:value={filterType} class="bg-zinc-950 border border-zinc-800 rounded px-3 py-1.5 text-sm text-zinc-200 focus:outline-none focus:border-emerald-500">
      <option value="">All Types</option>
      <option value="decision">Decision</option>
      <option value="memory_created">Memory Created</option>
      <option value="commit">Commit</option>
      <option value="error">Error</option>
    </select>
  </div>

  <div class="space-y-6">
    {#each sessions as session (session.id)}
      {#if eventsBySession[session.id] && eventsBySession[session.id].length > 0}
        <div class="border border-zinc-800 rounded-lg overflow-hidden bg-zinc-950/30">
          <button class="w-full text-left p-4 bg-zinc-900/50 hover:bg-zinc-800/50 flex justify-between items-center transition" onclick={() => toggleSession(session.id)}>
            <div>
              <div class="flex items-center gap-3">
                <h3 class="font-semibold text-white">Session {session.id}</h3>
                <span class="text-xs bg-zinc-800 text-zinc-300 px-2 py-0.5 rounded-full">{session.agent}</span>
                <span class="text-xs bg-indigo-900/40 text-indigo-300 border border-indigo-500/20 px-2 py-0.5 rounded-full">{session.appId}</span>
              </div>
              <p class="text-xs text-zinc-500 mt-1">Started: {new Date(session.startTime).toLocaleString()} • {session.eventCount} events</p>
            </div>
            <div class="text-zinc-500">
              {expandedSessions.has(session.id) ? '▲' : '▼'}
            </div>
          </button>
          
          {#if expandedSessions.has(session.id)}
            <div class="p-4 pl-8 border-t border-zinc-800 relative">
              <div class="absolute left-6 top-4 bottom-4 w-px bg-zinc-800"></div>
              <div class="space-y-4">
                {#each eventsBySession[session.id] as event (event.id)}
                  <div class="relative pl-6">
                    <div class="absolute -left-2 top-2 w-3 h-3 rounded-full bg-zinc-900 border-2 border-zinc-600 z-10 {getTypeColor(event.type).split(' ')[0]}"></div>
                    
                    <button class="w-full text-left bg-zinc-900/30 hover:bg-zinc-800/30 p-3 rounded-lg border border-zinc-800/50 transition" onclick={() => toggleEventContext(event.id)}>
                      <div class="flex justify-between items-start mb-2">
                        <div class="flex items-center gap-2">
                          <span class="text-xs font-medium border px-1.5 py-0.5 rounded-md {getTypeColor(event.type)}">{event.type}</span>
                          <span class="text-sm font-medium text-zinc-200">{event.agent}</span>
                        </div>
                        <span class="text-xs text-zinc-500">{new Date(event.timestamp).toLocaleTimeString()}</span>
                      </div>
                      
                      <p class="text-sm text-zinc-400 mb-3">{event.summary}</p>
                      
                      {#if event.entities && event.entities.length > 0}
                        <div class="flex flex-wrap gap-1.5">
                          {#each event.entities as entity}
                            <span class="text-[10px] bg-zinc-800 text-zinc-400 px-1.5 py-0.5 rounded border border-zinc-700">{entity}</span>
                          {/each}
                        </div>
                      {/if}
                    </button>

                    {#if expandedEvents[event.id]}
                      <div class="mt-2 ml-4 p-3 bg-zinc-900/50 border border-zinc-800 rounded-md text-sm text-zinc-400">
                        <pre class="font-mono text-xs overflow-x-auto">{JSON.stringify(expandedEvents[event.id], null, 2)}</pre>
                      </div>
                    {/if}
                  </div>
                {/each}
              </div>
            </div>
          {/if}
        </div>
      {/if}
    {/each}
  </div>
</div>
