import type { Agent, AgentEvent, EventWatcher, WatchEventsOptions } from "../types.mjs";


export function watch(agent: Agent<object>, options?: watch.Options): watch.ReturnType;
export declare namespace watch {
  type OnEventFn = (event: AgentEvent) => void;
  type Options = WatchEventsOptions;
  type ReturnType = EventWatcher;
  type Watcher = EventWatcher;
}
