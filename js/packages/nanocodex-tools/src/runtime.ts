import { createComputerRuntimeWithoutPdf, type ComputerRuntimeOptions } from "./runtime-core.js";
import { createPdfTextCommand } from "./pdf.js";

export type { ComputerCommandContext, ComputerRuntime, ComputerRuntimeOptions } from "./runtime-core.js";


export function createComputerRuntime(options: ComputerRuntimeOptions) {
  return createComputerRuntimeWithoutPdf({
    ...options,
    commands: context => [createPdfTextCommand(context.filesystem), ...(options.commands?.(context) ?? [])],
  });
}
