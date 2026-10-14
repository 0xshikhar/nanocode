export type {
  DurabilityAcquiredState,
  DurabilityAcquireRequest,
  DurabilityReplaceRequest,
  DurabilityReplaceResult,
  DurabilityFence,
  DurabilityExportCursor,
  DurabilityExportPageRequest,
  DurabilityPortableStatePage,
  DurabilityPortableStateArchive,
  DurabilityPortableStore,
  DurabilityRevision,
  DurabilitySqliteQuery,
  DurabilitySqliteRow,
  DurabilitySqliteTransaction,
  DurabilitySqliteValue,
  DurabilityStore,
  DurabilityStoredState,
  MemoryDurabilityStore,
  SqliteDurabilityStoreOptions,
} from "../types.mjs";

export declare const sqliteDurabilitySchema: readonly string[];

export declare class DurabilityImportConflictError extends Error {
  override readonly name: "DurabilityImportConflictError";
  readonly expectedRevision: import("../types.mjs").DurabilityRevision | undefined;
  readonly actualRevision: import("../types.mjs").DurabilityRevision | undefined;
  constructor(
    stateId: string,
    expectedRevision?: import("../types.mjs").DurabilityRevision,
    actualRevision?: import("../types.mjs").DurabilityRevision,
  );
}

export declare function durabilityRevision(

  value: string | bigint | number,
): import("../types.mjs").DurabilityRevision;


export declare function durabilityStateDigest(
  state: import("../types.mjs").DurabilityStoredState,
): Promise<string>;





export declare function exportDurabilityState(
  store: import("../types.mjs").DurabilityStore,
  stateId: string,
): Promise<import("../types.mjs").DurabilityPortableStateArchive>;


export declare function importDurabilityState(
  store: import("../types.mjs").DurabilityPortableStore,
  archive: import("../types.mjs").DurabilityPortableStateArchive,
): Promise<import("../types.mjs").DurabilityStoredState>;






export declare function exportDurabilityStatePage(
  store: import("../types.mjs").DurabilityStore,
  stateId: string,
  request: import("../types.mjs").DurabilityExportPageRequest,
): Promise<import("../types.mjs").DurabilityPortableStatePage>;


export declare function importDurabilityStatePages(
  store: import("../types.mjs").DurabilityPortableStore,
  pages: Iterable<import("../types.mjs").DurabilityPortableStatePage> | AsyncIterable<import("../types.mjs").DurabilityPortableStatePage>,
): Promise<import("../types.mjs").DurabilityStoredState>;

export declare function createMemoryDurabilityStore(
  stateId: string,
  initial?: import("../types.mjs").DurabilityStoredState,
): import("../types.mjs").MemoryDurabilityStore;

export declare function createSqliteDurabilityStore(
  options: import("../types.mjs").SqliteDurabilityStoreOptions,
): import("../types.mjs").DurabilityPortableStore;
