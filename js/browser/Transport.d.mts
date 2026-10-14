import type {
  ChatGptSubscriptionHandle,
  MppSession,
} from "../types.mjs";
import type {
  BrowserHttpRequest,
  BrowserWebSocketConnection,
  BrowserWebSocketRequest,
} from "./host.mjs";

declare const responsesTransport: unique symbol;
declare const workerTransport: unique symbol;

export type ResponsesTransport = Readonly<{
  [responsesTransport]: true;
}>;
export type Transport = ResponsesTransport;


export type WorkerTransport = ResponsesTransport & Readonly<{
  [workerTransport]: true;
}>;

type SharedEndpointOptions = Readonly<{
  apiBaseUrl?: string | undefined;

  stateless?: boolean | undefined;
  websocketUrl?: string | undefined;

  websocketPreconnect?: boolean | undefined;
  websocketWarmup?: boolean | undefined;
}>;

type WorkerEndpointOptions = SharedEndpointOptions & Readonly<{
  WebSocketImpl?: never;
  createWebSocket?: never;
  createResponse?: never;
}>;

type EndpointOptions = SharedEndpointOptions & Readonly<{
  createResponse?(endpoint: string, sessionId: string, request: BrowserHttpRequest): Promise<Response>;
  WebSocketImpl?: typeof WebSocket | undefined;
  createWebSocket?(
    endpoint: string,
    sessionId: string,
    request: BrowserWebSocketRequest,
  ): WebSocket | BrowserWebSocketConnection | Promise<WebSocket | BrowserWebSocketConnection>;
}>;

export function openAi(options: WorkerEndpointOptions & Readonly<{
  apiKey: string;
}>): WorkerTransport;
export function openAi(options: EndpointOptions & Readonly<{
  apiKey: string;
}>): ResponsesTransport;

export function chatGpt(options: EndpointOptions & Readonly<{
  subscription: ChatGptSubscriptionHandle;
}>): ResponsesTransport;


export function hostManaged(options?: WorkerEndpointOptions): WorkerTransport;
export function hostManaged(options?: EndpointOptions): ResponsesTransport;

export function mpp(options: EndpointOptions & Readonly<{
  session: MppSession;
}>): ResponsesTransport;




export function multiplex(socket: WebSocket): Readonly<{
  lane(streamId: string): WebSocket;
  close(code?: number, reason?: string): void;
}>;

export { anthropic, type ClaudeTransport } from "../runtime/claude-transport.mjs";
