import { invoke } from "@tauri-apps/api/core";

export async function pingKernel(): Promise<string> {
  return invoke<string>("ping_kernel");
}

export async function shutdownKernel(): Promise<string> {
  return invoke<string>("shutdown_kernel");
}
