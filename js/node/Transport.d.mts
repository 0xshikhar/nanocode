import type {
  ChatGptSubscriptionHandle,
  MppSession,
} from "../types.mjs";

declare const responsesTransport: unique symbol;

export type ResponsesTransport = Readonly<{
  [responsesTransport]: true;
}>;
export type Transport = ResponsesTransport;

type EndpointOptions = Readonly<{
  apiBaseUrl?: string | undefined;
  websocketUrl?: string | undefined;
  websocketWarmup?: boolean | undefined;
}>;

export function openAi(options: EndpointOptions & Readonly<{
  apiKey: string;
}>): ResponsesTransport;

export function chatGpt(options: EndpointOptions & Readonly<{
  subscription: ChatGptSubscriptionHandle;
}>): ResponsesTransport;

export function mpp(options: EndpointOptions & Readonly<{
  session: MppSession;
}>): ResponsesTransport;



export { anthropic, type ClaudeTransport } from "../runtime/claude-transport.mjs";
