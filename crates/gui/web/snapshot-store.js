const DATABASE = "neknaj-circuitgame2-gui-snapshots";
const VERSION = 1;
const CONTENT = "content";
const METADATA = "metadata";
const BINARY_TYPE = "application/octet-stream";

class SnapshotStoreError extends Error {
  constructor(code, message, cause) {
    super(message, { cause });
    this.code = code;
  }
}

function openDatabase() {
  return new Promise((resolve, reject) => {
    if (!globalThis.indexedDB) {
      reject(new SnapshotStoreError("unavailable", "Snapshot storage is unavailable in this browser."));
      return;
    }

    const request = globalThis.indexedDB.open(DATABASE, VERSION);
    let blocked = false;
    request.onupgradeneeded = () => {
      const database = request.result;
      database.createObjectStore(CONTENT, { autoIncrement: true });
      database.createObjectStore(METADATA, { keyPath: "id" });
    };
    request.onblocked = () => {
      blocked = true;
      reject(new SnapshotStoreError("unavailable", "Snapshot storage is locked by another page."));
    };
    request.onerror = () => reject(request.error);
    request.onsuccess = () => {
      const database = request.result;
      if (blocked) {
        database.close();
        return;
      }
      database.onversionchange = () => database.close();
      resolve(database);
    };
  });
}

async function runTransaction(stores, mode, enqueue) {
  let database;
  try {
    database = await openDatabase();
    const transaction = database.transaction(stores, mode);
    return await new Promise((resolve, reject) => {
      let result;
      let failure;
      // 書き込み要求が成功しても、トランザクションは後で失敗することがある。
      transaction.oncomplete = () => resolve(result);
      transaction.onabort = () => reject(failure ?? transaction.error ?? new DOMException("Transaction aborted.", "AbortError"));
      const setResult = (value) => { result = value; };
      const abort = (error) => {
        failure = error;
        transaction.abort();
      };
      try {
        enqueue(transaction, setResult, abort);
      } catch (error) {
        abort(error);
      }
    });
  } catch (error) {
    if (error instanceof SnapshotStoreError) throw error;
    if (error?.name === "QuotaExceededError") {
      throw new SnapshotStoreError("quota", "Snapshot storage is full. Export the snapshot to a file instead.", error);
    }
    if (["SecurityError", "NotAllowedError", "InvalidStateError", "VersionError"].includes(error?.name)) {
      throw new SnapshotStoreError("unavailable", "Snapshot storage is unavailable in this browser.", error);
    }
    if (mode === "readwrite") {
      throw new SnapshotStoreError("save_failed", "Could not save the snapshot in browser storage.", error);
    }
    throw new SnapshotStoreError("read_failed", "Could not read snapshots from browser storage.", error);
  } finally {
    database?.close();
  }
}

/**
 * バイト列を保存し、保存が確定した後に識別子を返す。
 * @param {Uint8Array} bytes
 * @returns {Promise<number>}
 */
export async function saveSnapshot(bytes) {
  const blob = new Blob([bytes], { type: BINARY_TYPE });
  const savedAt = Date.now();
  return runTransaction([CONTENT, METADATA], "readwrite", (transaction, setResult, abort) => {
    const body = transaction.objectStore(CONTENT).add(blob);
    body.onsuccess = () => {
      try {
        const id = body.result;
        transaction.objectStore(METADATA).add({ id, savedAt, size: blob.size });
        setResult(id);
      } catch (error) {
        abort(error);
      }
    };
  });
}

/**
 * 一覧表示用の記録だけを新しい順に返す。
 * @returns {Promise<Array<{id: number, savedAt: number, size: number}>>}
 */
export async function listSnapshots() {
  const entries = await runTransaction([METADATA], "readonly", (transaction, setResult) => {
    const request = transaction.objectStore(METADATA).getAll();
    request.onsuccess = () => setResult(request.result);
  });
  return entries.sort((left, right) => right.savedAt - left.savedAt || right.id - left.id);
}

/**
 * 復元用のファイルを取得する。記録が消えていた場合は not_found を返す。
 * @param {number} id
 * @returns {Promise<File>}
 */
export async function readSnapshot(id) {
  const missing = () => new SnapshotStoreError("not_found", "The saved snapshot no longer exists.");
  if (!Number.isInteger(id) || id < 1) throw missing();

  const blob = await runTransaction([CONTENT], "readonly", (transaction, setResult) => {
    const request = transaction.objectStore(CONTENT).get(id);
    request.onsuccess = () => setResult(request.result);
  });
  if (blob === undefined) throw missing();
  return new File([blob], "ncg2-snapshot.bin", { type: blob.type });
}
