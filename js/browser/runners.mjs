export function connectRunnerSocket({ url, token }) {
  return token === undefined ? new WebSocket(url) : new WebSocket(url, ["bearer", token]);
}
