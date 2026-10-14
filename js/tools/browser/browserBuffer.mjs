import * as bufferModule from "buffer";




const buffer = bufferModule.Buffer === undefined ? bufferModule.default : bufferModule;
export const { Buffer } = buffer;
if (typeof Buffer !== "function") {
  throw new Error("the browser Buffer compatibility module did not expose Buffer");
}


globalThis.Buffer ??= Buffer;


const commonJsGlobal = globalThis;
commonJsGlobal.global ??= globalThis;
commonJsGlobal.process ??= { versions: {} };
