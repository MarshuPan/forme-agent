// One in-memory command owner survives React re-renders and view navigation.
// It is discarded on sign-out. Nothing here is a persistent offline queue.
import { newKey } from './api/client.ts';
import type { WorkbenchClient } from './api/client.ts';
import type { ArtifactUploadReceipt, RunCommand, WorkbenchSessionRecord } from './types.ts';

type Client = Pick<WorkbenchClient, 'createWorkbenchSession' | 'submit' | 'uploadArtifact'>;
interface Turn { key: string; session: string | null; input: string; profile: string }
interface Upload { key: string; session: string | null; file: File }

export class ConversationCommands {
  private readonly confirmedSession: (record: WorkbenchSessionRecord) => void;
  constructor(confirmedSession: (record: WorkbenchSessionRecord) => void = () => {}) { this.confirmedSession = confirmedSession; }
  private disposed = false;
  private locked = false;
  private turn: Turn | null = null;
  private upload: Upload | null = null;
  private creation: { key: string; title: string } | null = null;
  get busy() { return this.locked; }
  get pending(): 'turn' | 'upload' | null { return this.turn ? 'turn' : this.upload ? 'upload' : null; }
  get pendingSession() { return this.turn?.session ?? this.upload?.session ?? null; }
  get pendingInput() { return this.turn?.input ?? null; }
  get pendingFilename() { return this.upload?.file.name ?? null; }
  dispose() { this.disposed = true; this.turn = null; this.upload = null; this.creation = null; }

  private async session(api: Client, item: Turn | Upload, title: string): Promise<string> {
    if (item.session) return item.session;
    this.creation ??= { key: newKey('session'), title: title.trim().slice(0, 64) || '新对话' };
    const created = await api.createWorkbenchSession(this.creation.title, null, this.creation.key);
    if (this.disposed) throw new Error('已退出当前会话，不再提交后续操作。');
    item.session = created.session;
    this.creation = null;
    this.confirmedSession(created);
    return created.session;
  }
  private requireActive() { if (this.disposed) throw new Error('已退出当前会话，不再提交后续操作。'); }

  submit(api: Client, session: string | null, input: string, profile: string): Promise<{ run: string; session: string }> | null {
    if (this.disposed || this.locked || this.upload || (!this.turn && !input.trim())) return null;
    this.turn ??= { key: newKey('command'), session, input, profile };
    this.locked = true;
    const item = this.turn;
    return (async () => {
      const activeSession = await this.session(api, item, item.input);
      this.requireActive();
      const command: RunCommand = { schema_version: 1, source: 'UserTurn', session: activeSession,
        input: item.input, agent_profile: item.profile, budget: null, idempotency_key: item.key };
      const run = await api.submit(command);
      this.turn = null;
      return { run, session: activeSession };
    })().finally(() => { this.locked = false; });
  }

  uploadFile(api: Client, session: string | null, file?: File): Promise<{ artifact: ArtifactUploadReceipt; session: string }> | null {
    if (this.disposed || this.locked || this.turn || (!this.upload && !file)) return null;
    this.upload ??= { key: newKey('artifact'), session, file: file! };
    this.locked = true;
    const item = this.upload;
    return (async () => {
      const activeSession = await this.session(api, item, item.file.name);
      this.requireActive();
      const artifact = await api.uploadArtifact(activeSession, null, item.file, item.key);
      this.upload = null;
      return { artifact, session: activeSession };
    })().finally(() => { this.locked = false; });
  }
}
