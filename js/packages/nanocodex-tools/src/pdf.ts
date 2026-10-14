import type { Workspace } from "../tools/types.mjs";
import { createPdfTextCommandWithExtractor } from "./pdf-command.js";


export function createPdfTextCommand(filesystem: () => Workspace) {
  return createPdfTextCommandWithExtractor(filesystem, async (data, options, signal) => {
    const { extractPdfText } = await import("./pdf-extract.js");
    return extractPdfText(data, options, signal);
  });
}
