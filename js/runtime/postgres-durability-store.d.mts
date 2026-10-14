import type { DurabilityPortableStore } from "../types.mjs";


export type PostgresDurabilityRow = Readonly<Record<string, unknown>>;


export type PostgresDurabilityQueryResult<
  Row extends PostgresDurabilityRow = PostgresDurabilityRow,
> = Readonly<{
  rows: readonly Row[];
}>;







export type PostgresDurabilityClient = Readonly<{
  query<Row extends PostgresDurabilityRow = PostgresDurabilityRow>(
    text: string,
    values?: unknown[] | undefined,
  ): PromiseLike<PostgresDurabilityQueryResult<Row>>;
  release(discard?: boolean | Error | undefined): void;
}>;








export type PostgresDurabilityPool = Readonly<{
  connect(): PromiseLike<PostgresDurabilityClient>;
  query<Row extends PostgresDurabilityRow = PostgresDurabilityRow>(
    text: string,
    values?: unknown[] | undefined,
  ): PromiseLike<PostgresDurabilityQueryResult<Row>>;
}>;





export declare class PostgresDurabilityUnavailableError extends Error {
  override readonly name: "PostgresDurabilityUnavailableError";
  constructor(stateId: string, cause: unknown);
}












export declare function createPostgresDurabilityStore(
  pool: PostgresDurabilityPool,
): DurabilityPortableStore;
