export type RunnerOptions = Readonly<{ url: string; token?: string; name?: string; mode?: "alongside" | "replace"; callTimeoutMs?: number }>;
export type RunnerServerOptions = Readonly<{ address?: string; token?: string; mode?: "alongside" | "replace"; callTimeoutMs?: number }>;
export type RunnerHandle = Readonly<{ name: string; workspace: string; mode: "alongside" | "replace"; status: "connected" | "disconnected"; disconnect(): Promise<void> }>;
export type RunnerServer = Readonly<{ address: string; url: string; token: string; stop(): Promise<void> }>;
export type RunnerEvent = Readonly<{ type: "connected" | "disconnected"; runner: RunnerHandle; message?: string }> | Readonly<{ type: "rejected"; message: string }>;
