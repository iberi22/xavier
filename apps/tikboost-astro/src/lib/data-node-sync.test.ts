import { describe, it, expect, beforeEach, vi } from 'vitest';
import { DataNodeSync, SyncedRecord, SyncKind, DataNodeClientInterface } from './data-node-sync.js';

class MockDataNodeClient implements DataNodeClientInterface {
  public remoteStore: Map<string, SyncedRecord> = new Map();
  public isOnline = true;

  private getKey(kind: SyncKind, id: string): string {
    return `${kind}:${id}`;
  }

  async pushRecord(kind: SyncKind, id: string, data: any, updatedAt: number): Promise<{ success: boolean }> {
    if (!this.isOnline) {
      throw new Error('Network error: node unreachable');
    }
    this.remoteStore.set(this.getKey(kind, id), { id, kind, data, updatedAt });
    return { success: true };
  }

  async deleteRecord(kind: SyncKind, id: string): Promise<{ success: boolean }> {
    if (!this.isOnline) {
      throw new Error('Network error: node unreachable');
    }
    this.remoteStore.delete(this.getKey(kind, id));
    return { success: true };
  }

  async fetchRecords(kind: SyncKind): Promise<SyncedRecord[]> {
    if (!this.isOnline) {
      throw new Error('Network error: node unreachable');
    }
    const result: SyncedRecord[] = [];
    for (const [key, value] of this.remoteStore.entries()) {
      if (key.startsWith(`${kind}:`)) {
        result.push(value);
      }
    }
    return result;
  }

  async healthCheck(): Promise<boolean> {
    return this.isOnline;
  }
}

describe('DataNodeSync', () => {
  let mockClient: MockDataNodeClient;
  let sync: DataNodeSync;

  beforeEach(() => {
    mockClient = new MockDataNodeClient();
    sync = new DataNodeSync({
      dataNodeClient: mockClient
    });
  });

  it('should push and pull across all 4 synced kinds', async () => {
    const kinds: SyncKind[] = ['overlay', 'streaming_session', 'session_analytics', 'settings'];

    for (const kind of kinds) {
      const res = await sync.push(kind, `test-${kind}-1`, { name: `Test ${kind}` }, 1000);
      expect(res.success).toBe(true);
      expect(res.queuedLocally).toBe(false);

      // Remote store should contain pushed record
      const remoteRecord = mockClient.remoteStore.get(`${kind}:test-${kind}-1`);
      expect(remoteRecord).toBeDefined();
      expect(remoteRecord?.data).toEqual({ name: `Test ${kind}` });

      // Pulling kind should return record
      const pulled = await sync.pull(kind);
      expect(pulled.length).toBeGreaterThanOrEqual(1);
      expect(pulled.find((r) => r.id === `test-${kind}-1`)?.data).toEqual({ name: `Test ${kind}` });
    }
  });

  it('E2E test: create overlay locally → sync → verify exists remotely → modify remotely → pull → local updated', async () => {
    const overlayId = 'widget-overlay-15';
    const initialOverlayData = { type: 'chat-widget', title: 'Stream Chat' };

    // 1. Create overlay locally & sync
    const pushRes = await sync.push('overlay', overlayId, initialOverlayData, 2000);
    expect(pushRes.success).toBe(true);

    // 2. Verify exists remotely
    const remoteKey = `overlay:${overlayId}`;
    let remoteRec = mockClient.remoteStore.get(remoteKey);
    expect(remoteRec).toBeDefined();
    expect(remoteRec?.data.title).toBe('Stream Chat');

    // 3. Modify remotely with a newer timestamp
    const updatedOverlayData = { type: 'chat-widget', title: 'Updated Stream Chat V2' };
    mockClient.remoteStore.set(remoteKey, {
      id: overlayId,
      kind: 'overlay',
      data: updatedOverlayData,
      updatedAt: 3000 // Newer timestamp
    });

    // 4. Pull remote changes
    const pulledRecords = await sync.pull('overlay');
    const localOverlay = pulledRecords.find((r) => r.id === overlayId);

    // 5. Verify local store updated with remote changes
    expect(localOverlay).toBeDefined();
    expect(localOverlay?.data.title).toBe('Updated Stream Chat V2');
    expect(localOverlay?.updatedAt).toBe(3000);
  });

  it('Offline simulation: mock fetch failure → write queued → restore → flush succeeds', async () => {
    const sessionId = 'session-99';
    const sessionData = { viewers: 1500, status: 'live' };

    // 1. Simulate offline state (node unreachable)
    mockClient.isOnline = false;

    const pushRes = await sync.push('streaming_session', sessionId, sessionData, 5000);
    expect(pushRes.success).toBe(false);
    expect(pushRes.queuedLocally).toBe(true);

    // Remote store should NOT have the record yet
    expect(mockClient.remoteStore.get(`streaming_session:${sessionId}`)).toBeUndefined();

    // Verify offline queue contains item
    const store = sync.getStore();
    const queue = await store.getOfflineQueue();
    expect(queue.length).toBe(1);
    expect(queue[0].id).toBe(sessionId);
    expect(queue[0].kind).toBe('streaming_session');

    // 2. Restore node connectivity
    mockClient.isOnline = true;

    // 3. Flush offline queue
    const flushRes = await sync.flushQueue();
    expect(flushRes.flushedCount).toBe(1);
    expect(flushRes.remainingCount).toBe(0);

    // Verify remote node now has the synced record
    const remoteRec = mockClient.remoteStore.get(`streaming_session:${sessionId}`);
    expect(remoteRec).toBeDefined();
    expect(remoteRec?.data).toEqual(sessionData);
    expect(remoteRec?.updatedAt).toBe(5000);
  });
});
