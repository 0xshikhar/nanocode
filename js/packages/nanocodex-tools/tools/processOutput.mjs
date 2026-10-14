import { mkdtemp, open, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { StringDecoder } from "node:string_decoder";

const MEMORY_BYTES = 64 * 1024;





export async function createProcessOutput() {
  let directory, file;
  let chunks = [];
  let fileBase = 0;
  let queued = Promise.resolve();
  let accepted = 0;
  let written = 0;
  let consumed = 0;
  let closing;
  const decoder = new StringDecoder("utf8");
  const enqueue = operation => {
    const result = queued.then(operation);
    queued = result.catch(() => {});
    return result;
  };
  const write = async (buffer, position) => {
    let offset = 0;
    while (offset < buffer.length) {
      const { bytesWritten } = await file.write(buffer, offset, buffer.length - offset, position + offset);
      if (!bytesWritten) throw new Error("Could not write process output.");
      offset += bytesWritten;
    }
  };
  const spill = async () => {
    directory = await mkdtemp(join(tmpdir(), "nanocodex-output-"));
    try { file = await open(join(directory, "output"), "wx+", 0o600); }
    catch (error) {
      await rm(directory, { recursive: true, force: true });
      directory = undefined;
      throw error;
    }

    fileBase = consumed;
    let position = 0;
    for (const chunk of chunks) {
      await write(chunk, position);
      position += chunk.length;
    }
    chunks = [];
  };
  return {
    get unread() { return accepted - consumed; },
    append(data) {
      if (closing) return Promise.reject(new Error("Process output is closed."));
      const buffer = Buffer.isBuffer(data) ? data : Buffer.from(data);
      accepted += buffer.length;
      return enqueue(async () => {
        if (!file && written - consumed + buffer.length > MEMORY_BYTES) await spill();
        if (file) await write(buffer, written - fileBase);
        else if (buffer.length) chunks.push(Buffer.from(buffer));
        written += buffer.length;
      });
    },
    read(maxBytes) {
      return enqueue(async () => {
        const buffer = Buffer.alloc(Math.min(maxBytes, written - consumed));
        let bytesRead = 0;
        if (file && buffer.length) {
          ({ bytesRead } = await file.read(buffer, 0, buffer.length, consumed - fileBase));
        } else {
          while (bytesRead < buffer.length) {
            const chunk = chunks[0];
            const take = Math.min(chunk.length, buffer.length - bytesRead);
            chunk.copy(buffer, bytesRead, 0, take);
            bytesRead += take;
            if (take === chunk.length) chunks.shift();


            else chunks[0] = chunk.subarray(take);
          }
        }
        consumed += bytesRead;
        return decoder.write(buffer.subarray(0, bytesRead));
      });
    },
    end() { return decoder.end(); },
    close() {
      return closing ??= enqueue(async () => {
        chunks = [];
        try { await file?.close(); }
        finally { if (directory) await rm(directory, { recursive: true, force: true }); }
      });
    },
  };
}
