/**
 * DataNodeSync — Bidirectional sync local IndexedDB ↔ remote node
 * Kinds synced: 'overlay', 'streaming_session', 'session_analytics', 'settings'
 * Sync strategy: Local is source of truth, push changes on save, pull on load if remote newer (LWW).
 * Offline queue: If node is unreachable, queue writes locally and flush later.
 */

export type SyncKind = 'overlay' | 'streaming_session' | 'session_analytics' | 'settings';

export const SYNC_KINDS: SyncKind[] = [
  'overlay',
  'streaming_session',
  'session_analytics',
  'settings'
];

export interface SyncedRecord<T = any> {
  id: string;
  kind: SyncKind;
  data: T;
  updatedAt: number;
}

export interface QueueItem {
  id: string;
  kind: SyncKind;
  data: any;
  updatedAt: number;
  action: 'save' | 'delete';
}

export interface LocalStoreInterface {
  getItem<T = any>(kind: SyncKind, id: string): Promise<SyncedRecord<T> | null>;
  setItem<T = any>(kind: SyncKind, id: string, data: T, updatedAt?: number): Promise<SyncedRecord<T>>;
  removeItem(kind: SyncKind, id: string): Promise<void>;
  getAll<T = any>(kind: SyncKind): Promise<SyncedRecord<T>[]>;
  getOfflineQueue(): Promise<QueueItem[]>;
  addToOfflineQueue(item: QueueItem): Promise<void>;
  removeFromOfflineQueue(id: string, kind: SyncKind): Promise<void>;
  clearOfflineQueue(): Promise<void>;
}

export interface DataNodeClientInterface {
  pushRecord(kind: SyncKind, id: string, data: any, updatedAt: number): Promise<{ success: boolean }>;
  deleteRecord?(kind: SyncKind, id: string): Promise<{ success: boolean }>;
  fetchRecords(kind: SyncKind, since?: number): Promise<SyncedRecord[]>;
  healthCheck(): Promise<boolean>;
}

export interface DataNodeSyncOptions {
  nodeUrl?: string;
  store?: LocalStoreInterface;
  dataNodeClient?: DataNodeClientInterface;
  fetchFn?: typeof fetch;
}

// In-memory fallback local store if none provided
class InMemoryLocalStore implements LocalStoreInterface {
  private records: Map<string, SyncedRecord> = new Map();
  private queue: QueueItem[] = [];

  private getKey(kind: SyncKind, id: string): string {
    return `${kind}:${id}`;
  }

  async getItem<T = any>(kind: SyncKind, id: string): Promise<SyncedRecord<T> | null> {
    const record = this.records.get(this.getKey(kind, id));
    return record ? (record as SyncedRecord<T>) : null;
  }

  async setItem<T = any>(kind: SyncKind, id: string, data: T, updatedAt?: number): Promise<SyncedRecord<T>> {
    const record: SyncedRecord<T> = {
      id,
      kind,
      data,
      updatedAt: updatedAt ?? Date.now()
    };
    this.records.set(this.getKey(kind, id), record);
    return record;
  }

  async removeItem(kind: SyncKind, id: string): Promise<void> {
    this.records.delete(this.getKey(kind, id));
  }

  async getAll<T = any>(kind: SyncKind): Promise<SyncedRecord<T>[]> {
    const result: SyncedRecord<T>[] = [];
    for (const [key, value] of this.records.entries()) {
      if (key.startsWith(`${kind}:`)) {
        result.push(value as SyncedRecord<T>);
      }
    }
    return result;
  }

  async getOfflineQueue(): Promise<QueueItem[]> {
    return [...this.queue];
  }

  async addToOfflineQueue(item: QueueItem): Promise<void> {
    // Replace if already queued for same id and kind
    this.queue = this.queue.filter((q) => !(q.id === item.id && q.kind === item.kind));
    this.queue.push(item);
  }

  async removeFromOfflineQueue(id: string, kind: SyncKind): Promise<void> {
    this.queue = this.queue.filter((q) => !(q.id === id && q.kind === kind));
  }

  async clearOfflineQueue(): Promise<void> {
    this.queue = [];
  }
}

export class DataNodeSync {
  private nodeUrl: string;
  private store: LocalStoreInterface;
  private dataNodeClient?: DataNodeClientInterface;
  private fetchFn: typeof fetch;

  constructor(options: DataNodeSyncOptions = {}) {
    this.nodeUrl = options.nodeUrl || 'http://localhost:8006';
    this.store = options.store || new InMemoryLocalStore();
    this.dataNodeClient = options.dataNodeClient;
    this.fetchFn = options.fetchFn || (typeof fetch !== 'undefined' ? fetch.bind(globalThis) : (async () => {
      throw new Error('fetch unavailable');
    }) as any);
  }

  public getStore(): LocalStoreInterface {
    return this.store;
  }

  /**
   * Push a record change locally and sync to remote node.
   * If remote node is unreachable, write is appended to offline queue.
   */
  async push<T = any>(kind: SyncKind, id: string, data: T, updatedAt?: number): Promise<{ success: boolean; queuedLocally: boolean; record: SyncedRecord<T> }> {
    const ts = updatedAt ?? Date.now();
    const localRecord = await this.store.setItem(kind, id, data, ts);

    try {
      if (this.dataNodeClient) {
        const res = await this.dataNodeClient.pushRecord(kind, id, data, ts);
        if (res.success) {
          return { success: true, queuedLocally: false, record: localRecord };
        }
      } else {
        const response = await this.fetchFn(`${this.nodeUrl}/v1/maloca/node/sync`, {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ kind, id, data, updatedAt: ts })
        });
        if (response.ok) {
          return { success: true, queuedLocally: false, record: localRecord };
        }
      }
    } catch (_err) {
      // Node unreachable or network failure — queue write locally
    }

    // Queue for later sync
    await this.store.addToOfflineQueue({
      id,
      kind,
      data,
      updatedAt: ts,
      action: 'save'
    });

    return { success: false, queuedLocally: true, record: localRecord };
  }

  /**
   * Pull changes from remote node for a given kind.
   * Remote record updates local store if remote.updatedAt > local.updatedAt (LWW).
   */
  async pull<T = any>(kind: SyncKind): Promise<SyncedRecord<T>[]> {
    let remoteRecords: SyncedRecord<T>[] = [];

    try {
      if (this.dataNodeClient) {
        remoteRecords = (await this.dataNodeClient.fetchRecords(kind)) as SyncedRecord<T>[];
      } else {
        const response = await this.fetchFn(`${this.nodeUrl}/v1/maloca/node/sync?kind=${encodeURIComponent(kind)}`);
        if (response.ok) {
          const body = await response.json();
          remoteRecords = (body.data || body.records || body) as SyncedRecord<T>[];
        }
      }
    } catch (_err) {
      // Remote unavailable, return local records as-is
      return this.store.getAll<T>(kind);
    }

    for (const remoteRec of remoteRecords) {
      const localRec = await this.store.getItem<T>(kind, remoteRec.id);
      if (!localRec || remoteRec.updatedAt > localRec.updatedAt) {
        await this.store.setItem(kind, remoteRec.id, remoteRec.data, remoteRec.updatedAt);
      }
    }

    return this.store.getAll<T>(kind);
  }

  /**
   * Pull remote changes across all 4 synced kinds.
   */
  async pullAll(): Promise<Record<SyncKind, SyncedRecord[]>> {
    const result = {} as Record<SyncKind, SyncedRecord[]>;
    for (const kind of SYNC_KINDS) {
      result[kind] = await this.pull(kind);
    }
    return result;
  }

  /**
   * Flush queued offline writes to remote node.
   */
  async flushQueue(): Promise<{ flushedCount: number; remainingCount: number }> {
    const queue = await this.store.getOfflineQueue();
    if (queue.length === 0) {
      return { flushedCount: 0, remainingCount: 0 };
    }

    let flushedCount = 0;

    for (const item of queue) {
      try {
        let success = false;
        if (this.dataNodeClient) {
          if (item.action === 'delete' && this.dataNodeClient.deleteRecord) {
            const res = await this.dataNodeClient.deleteRecord(item.kind, item.id);
            success = res.success;
          } else {
            const res = await this.dataNodeClient.pushRecord(item.kind, item.id, item.data, item.updatedAt);
            success = res.success;
          }
        } else {
          const response = await this.fetchFn(`${this.nodeUrl}/v1/maloca/node/sync`, {
            method: item.action === 'delete' ? 'DELETE' : 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ kind: item.kind, id: item.id, data: item.data, updatedAt: item.updatedAt })
          });
          success = response.ok;
        }

        if (success) {
          await this.store.removeFromOfflineQueue(item.id, item.kind);
          flushedCount++;
        }
      } catch (_err) {
        // Stop flushing on network error, keep remaining items queued
        break;
      }
    }

    const remainingQueue = await this.store.getOfflineQueue();
    return { flushedCount, remainingCount: remainingQueue.length };
  }

  /**
   * Full sync: flush offline queue, then pull all remote changes.
   */
  async syncAll(): Promise<{ flushedCount: number; records: Record<SyncKind, SyncedRecord[]> }> {
    const { flushedCount } = await this.flushQueue();
    const records = await this.pullAll();
    return { flushedCount, records };
  }
}
