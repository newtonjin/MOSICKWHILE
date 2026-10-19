declare module "onnxruntime-web" {
  // Minimal shim — runtime types come from the package; Vite resolves the ESM build.
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  const ort: any;
  export = ort;
}
