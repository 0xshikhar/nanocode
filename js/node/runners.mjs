import WebSocket, { WebSocketServer } from "ws";
import { createServer } from "node:http";
import { randomBytes, timingSafeEqual } from "node:crypto";
import { createNodeProcessTools } from "nanocodex-tools/node";

export function connectRunnerSocket({ url, token }) {
  return new WebSocket(url, { headers: token === undefined ? {} : { Authorization: "Bearer " + token } });
}

export async function listenRunnerSockets({ address = "127.0.0.1:0", token = randomBytes(32).toString("base64url") }, accept) {
  if (typeof token !== "string" || !token) throw new TypeError("runner server token must not be empty");
  const target = new URL("http://" + address);
  const server = createServer((request, response) => { response.writeHead(404); response.end(); });
  const sockets = new WebSocketServer({ noServer: true });
  const authenticate = request => {
    const actual = Buffer.from(request.headers.authorization ?? "");
    const expected = Buffer.from("Bearer " + token);
    return actual.length === expected.length && timingSafeEqual(actual, expected);
  };
  server.on("upgrade", (request, socket, head) => {
    if (!authenticate(request)) { socket.end("HTTP/1.1 401 Unauthorized\r\nConnection: close\r\n\r\n"); return; }
    sockets.handleUpgrade(request, socket, head, accept);
  });
  await new Promise((resolve, reject) => { server.once("error", reject); server.listen(Number(target.port), target.hostname, resolve); });
  const bound = server.address();
  const host = bound.address.includes(":") ? "[" + bound.address + "]" : bound.address;
  let stopping;
  return { address: host + ":" + bound.port, url: "ws://" + host + ":" + bound.port, token, stop() { return stopping ??= new Promise((resolve, reject) => { for (const socket of sockets.clients) socket.terminate(); sockets.close(); server.close(error => error ? reject(error) : resolve()); }); } };
}

export async function runnerProcessTools(workspace) {
  const instance = await createNodeProcessTools({ workspace });
  return { tools: Object.fromEntries(instance.tools.map(({ name, ...tool }) => [name, tool])), close: instance.close };
}
