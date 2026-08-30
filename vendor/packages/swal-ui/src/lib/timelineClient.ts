export interface TimelineEvent {
  id: string;
  sessionId: string;
  timestamp: string;
  agent: string;
  type: 'decision' | 'memory_created' | 'commit' | 'error';
  summary: string;
  entities: string[];
  context?: any;
  appId?: string;
}

export interface TimelineSession {
  id: string;
  startTime: string;
  endTime?: string;
  agent: string;
  appId: string;
  eventCount: number;
  keyDecisions: string[];
}

export async function fetchTimeline(): Promise<TimelineEvent[]> {
  try {
    const res = await fetch('/maloca/timeline');
    if (!res.ok) return [];
    return res.json();
  } catch {
    return [];
  }
}

export async function fetchTimelineSessions(): Promise<TimelineSession[]> {
  try {
    const res = await fetch('/maloca/timeline/sessions');
    if (!res.ok) return [];
    return res.json();
  } catch {
    return [];
  }
}

export async function fetchEventContext(id: string): Promise<any> {
  try {
    const res = await fetch(`/maloca/timeline/${id}/context`);
    if (!res.ok) return null;
    return res.json();
  } catch {
    return null;
  }
}
