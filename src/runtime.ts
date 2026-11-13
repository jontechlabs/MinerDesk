import { invoke } from "@tauri-apps/api/core";
import { createNativeJsonClient, type NativeJsonRequest } from "./api";

declare global {
  interface Window { __MINERDESK_MOBILE__?: boolean }
}

// Set by the separate bundled mobile entry before importing the shared app.
export const isMobileClient = typeof window !== "undefined" && window.__MINERDESK_MOBILE__ === true;
export const isDesktopTauri = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window && !isMobileClient;

export type MobileSession = {
  connected: boolean;
  connecting?: boolean;
  connectionId?: string;
  address?: string;
  name?: string;
  version?: string;
};

export const mobileConnectionStatus = () => invoke<MobileSession>("mobile_connection_status");
export const mobileConnect = (address: string, key: string, allowInsecureLan: boolean) =>
  invoke<MobileSession>("mobile_connect", { address, key, allowInsecureLan });
export const mobileDisconnect = () => invoke<void>("mobile_disconnect");
export function createMobileApi(connectionId: string) {
  return createNativeJsonClient(<T,>(request: NativeJsonRequest) =>
    invoke<T>("mobile_request", { ...request }), connectionId);
}
