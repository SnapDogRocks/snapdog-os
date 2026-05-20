async function request<T>(url: string, options?: RequestInit): Promise<T> {
  const res = await fetch(url, {
    headers: { "Content-Type": "application/json" },
    ...options,
  });
  if (!res.ok) throw new Error(`API error: ${res.status} ${res.statusText}`);
  return res.json();
}

// ── Types ─────────────────────────────────────────────────────

export interface SystemInfo {
  hostname: string;
  version: string;
  channel: string;
  uptime_seconds: number;
  pi_version: number;
  components: { client: string; ctrl: string; kernel: string };
}

export interface WifiNetwork {
  ssid: string;
  signal: number;
  security: string;
}

export interface NetworkConfig {
  mode: "dhcp" | "static";
  ip?: string;
  subnet?: string;
  gateway?: string;
  dns?: string;
}

export interface WifiStatus {
  connected: boolean;
  ssid: string;
  ip: string;
  subnet: string;
  gateway: string;
  dns: string;
  signal: number;
  mode: "dhcp" | "static";
}

export interface EthernetStatus {
  connected: boolean;
  mode: "dhcp" | "static";
  ip: string;
  subnet: string;
  gateway: string;
  dns: string;
}

export interface DacOverlay {
  id: string;
  name: string;
}

export interface AudioConfig {
  overlay: string;
  detected_card: string;
  soundcard: string;
  available_overlays: DacOverlay[];
}

export interface ClientConfig {
  server_url: string;
  host_id: string;
  soundcard: string;
  mixer: string;
  latency: number;
  available_soundcards?: string[];
}

export interface SshConfig {
  enabled: boolean;
  pubkeys: string[];
}

export interface UpdateCheck {
  available: boolean;
  installable: boolean;
  current_version: string;
  latest_version: string;
  channel: string;
  is_downgrade: boolean;
}

export interface UpdateStatus {
  phase: "idle" | "installing";
  progress: number | null;
  rolled_back: boolean;
}

// ── API calls ─────────────────────────────────────────────────

export const api = {
  getSystem: () => request<SystemInfo>("/api/system"),
  setSystem: (data: { hostname?: string; channel?: string }) =>
    request<void>("/api/system", { method: "PUT", body: JSON.stringify(data) }),
  reboot: () => request<void>("/api/system/reboot", { method: "POST" }),
  triggerUpdate: () => request<void>("/api/system/update", { method: "POST" }),
  checkUpdate: () => request<UpdateCheck>("/api/system/update/check"),
  getUpdateStatus: () => request<import("./api").UpdateStatus>("/api/system/update/status"),
  factoryReset: () => request<void>("/api/system/factory-reset", { method: "POST" }),

  getEthernet: () => request<EthernetStatus>("/api/network/ethernet"),
  setEthernet: (config: NetworkConfig) =>
    request<void>("/api/network/ethernet", { method: "PUT", body: JSON.stringify(config) }),
  getWifi: () => request<WifiStatus>("/api/network/wifi"),
  scanWifi: () => request<{ networks: WifiNetwork[] }>("/api/network/wifi/scan", { method: "POST" }),
  setWifi: (config: { ssid: string; password: string; mode?: "dhcp" | "static"; ip?: string; subnet?: string; gateway?: string; dns?: string }) =>
    request<void>("/api/network/wifi", { method: "PUT", body: JSON.stringify(config) }),
  disconnectWifi: () => request<void>("/api/network/wifi", { method: "DELETE" }),

  getAudio: () => request<AudioConfig>("/api/audio"),
  setAudio: (overlay: string) =>
    request<void>("/api/audio", { method: "PUT", body: JSON.stringify({ overlay }) }),

  getClient: () => request<ClientConfig>("/api/client"),
  setClient: (config: ClientConfig) =>
    request<void>("/api/client", { method: "PUT", body: JSON.stringify(config) }),
  scanServers: () => request<{ servers: { name: string; host: string; port: number }[] }>("/api/client/scan-servers", { method: "POST" }),
  testServer: (host: string, port: number) => request<{ reachable: boolean }>("/api/client/test-server", { method: "POST", body: JSON.stringify({ host, port }) }),

  getSsh: () => request<SshConfig>("/api/ssh"),
  setSsh: (config: SshConfig) =>
    request<void>("/api/ssh", { method: "PUT", body: JSON.stringify(config) }),
};
