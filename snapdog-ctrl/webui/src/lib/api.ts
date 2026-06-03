const TOKEN_KEY = "snapdog_auth_token";

function getToken(): string | null {
  if (typeof window === "undefined") return null;
  return localStorage.getItem(TOKEN_KEY);
}

function setToken(token: string): void {
  localStorage.setItem(TOKEN_KEY, token);
}

function clearToken(): void {
  localStorage.removeItem(TOKEN_KEY);
}

async function request<T>(url: string, options?: RequestInit): Promise<T> {
  const headers: Record<string, string> = { "Content-Type": "application/json" };
  const token = getToken();
  if (token) headers["Authorization"] = `Bearer ${token}`;

  const res = await fetch(url, { headers, ...options });
  if (res.status === 401) {
    clearToken();
    window.dispatchEvent(new Event("snapdog-auth-expired"));
    throw new Error("Unauthorized");
  }
  if (!res.ok) throw new Error(`API error: ${res.status} ${res.statusText}`);
  return res.json();
}

// ── Types ─────────────────────────────────────────────────────

export interface SystemInfo {
  hostname: string;
  version: string;
  channel: string;
  uptime_seconds: number;
  board_model: string;
  components: { server: string; client: string; ctrl: string; kernel: string };
}

export interface TuningConfig {
  rf_kill_wifi: boolean;
  rf_kill_bluetooth: boolean;
  disable_onboard_audio: boolean;
  exclusive_audio_core: boolean;
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
  detected_hat: string;
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
  signature_verified: boolean;
}

export interface UpdateStatus {
  phase: "idle" | "installing";
  progress: number | null;
  rolled_back: boolean;
}

export interface ServerConfig {
  name: string;
  http: { api_keys: string[] };
  audio: { sample_rate: number; bit_depth: number; channels: number; source_conflict: string; zone_switch_fade_ms: number; source_switch_fade_ms: number };
  snapcast: { streaming_port: number; codec: string; encryption_psk: string | null; group_volume_mode: string; unknown_clients: string; default_zone: string; mdns_name: string; advertise_snapcast: boolean };
  subsonic: { url: string; username: string; password: string; format: string } | null;
  spotify: { name: string; bitrate: number } | null;
  airplay: { password: string | null; mode: string } | null;
  mqtt: { broker: string; username: string | null; password: string | null; base_topic: string } | null;
  knx: { role: string; url: string | null; gos?: { target: string; function: string; ga: string }[] } | null;
  zones: { name: string; icon: string }[];
  clients: { name: string; mac: string; zone: string; icon: string; max_volume: number }[];
  radio: { name: string; url: string; cover: string | null }[];
  system: { log_level: string };
}

export interface ServerStatus { enabled: boolean; running: boolean }

// ── API calls ─────────────────────────────────────────────────

export const api = {
  getSystem: () => request<SystemInfo>("/api/system"),
  setSystem: (data: { hostname?: string; channel?: string }) =>
    request<void>("/api/system", { method: "PUT", body: JSON.stringify(data) }),
  getTuning: () => request<TuningConfig>("/api/system/tuning"),
  setTuning: (config: TuningConfig) =>
    request<void>("/api/system/tuning", { method: "PUT", body: JSON.stringify(config) }),
  triggerUpdate: () => request<void>("/api/system/update", { method: "POST" }),
  checkUpdate: () => request<UpdateCheck>("/api/system/update/check"),
  getUpdateStatus: () => request<import("./api").UpdateStatus>("/api/system/update/status"),
  uploadUpdate: (file: File) => {
    const formData = new FormData();
    formData.append("file", file);
    return fetch("/api/system/update/upload", {
      method: "POST",
      body: formData,
    }).then(res => {
      if (!res.ok) throw new Error(`Upload failed: ${res.status} ${res.statusText}`);
    });
  },
  installUpdate: () => request<void>("/api/system/update/install", { method: "POST" }),
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

  getServer: () => request<ServerConfig>("/api/server"),
  setServer: (config: ServerConfig) => request<void>("/api/server", { method: "PUT", body: JSON.stringify(config) }),
  getServerStatus: () => request<ServerStatus>("/api/server/status"),
  enableServer: () => request<void>("/api/server/enable", { method: "POST" }),
  disableServer: () => request<void>("/api/server/disable", { method: "POST" }),

  // Auth
  getAuthStatus: () => request<AuthStatus>("/api/auth/status"),
  login: async (password: string): Promise<boolean> => {
    try {
      const res = await request<{ token: string }>("/api/auth/login", {
        method: "POST",
        body: JSON.stringify({ password }),
      });
      setToken(res.token);
      return true;
    } catch {
      return false;
    }
  },
  logout: async (): Promise<void> => {
    try { await request<void>("/api/auth/logout", { method: "POST" }); } catch { /* ignore */ }
    clearToken();
  },
  setPassword: (current: string | null, newPassword: string | null) =>
    request<void>("/api/auth/password", {
      method: "PUT",
      body: JSON.stringify({ current, new: newPassword }),
    }),

  // SoftAP
  getSoftap: () => request<{ enabled: boolean; password: string }>("/api/network/softap"),
  setSoftap: (config: { enabled: boolean; password: string }) =>
    request<void>("/api/network/softap", { method: "PUT", body: JSON.stringify(config) }),

  // Timezone
  getTimezone: () => request<{ timezone: string; available: string[] }>("/api/system/timezone"),
  setTimezone: (timezone: string) =>
    request<void>("/api/system/timezone", { method: "PUT", body: JSON.stringify({ timezone }) }),

  // Auto-Update
  getAutoUpdate: () => request<{ enabled: boolean; channel: string; interval: string; time: string }>("/api/system/update/auto"),
  setAutoUpdate: (config: { enabled: boolean; channel: string; interval: string; time: string }) =>
    request<void>("/api/system/update/auto", { method: "PUT", body: JSON.stringify(config) }),

  // Raw Flash
  flashRawUpload: async (file: File): Promise<{ challenge: string; expires_in_seconds: number }> => {
    const form = new FormData();
    form.append("file", file);
    const headers: Record<string, string> = {};
    const token = getToken();
    if (token) headers["Authorization"] = `Bearer ${token}`;
    const res = await fetch("/api/system/update/flash-raw", { method: "POST", headers, body: form });
    if (!res.ok) throw new Error(`Upload failed: ${res.status}`);
    return res.json();
  },
  flashRawConfirm: (challenge: string) =>
    request<void>("/api/system/update/flash-raw/confirm", { method: "POST", body: JSON.stringify({ challenge }) }),

  // Now Playing
  getNowPlaying: () => request<import("./api").NowPlaying>("/api/now-playing"),
  nowPlayingCommand: (command: string) =>
    request<void>("/api/now-playing/command", { method: "POST", body: JSON.stringify({ command }) }),
  setNowPlayingVolume: (volume: number) =>
    request<void>("/api/now-playing/volume", { method: "PUT", body: JSON.stringify({ volume }) }),
  nowPlayingSeek: (offset_ms: number) =>
    request<void>("/api/now-playing/seek", { method: "POST", body: JSON.stringify({ offset_ms }) }),

  // Health
  getHealth: () => request<{ ok: boolean; warnings: { id: string; severity: string }[] }>("/api/system/health"),

  // Reboot
  reboot: () => request<void>("/api/system/reboot", { method: "POST" }),

  // Settings export/import
  exportSettings: async (): Promise<Blob> => {
    const headers: Record<string, string> = {};
    const token = getToken();
    if (token) headers["Authorization"] = `Bearer ${token}`;
    const res = await fetch("/api/settings/export", { headers });
    if (!res.ok) throw new Error(`Export failed: ${res.status}`);
    return res.blob();
  },
  previewSettings: async (file: File): Promise<SettingsPreview> => {
    const headers: Record<string, string> = {};
    const token = getToken();
    if (token) headers["Authorization"] = `Bearer ${token}`;
    const res = await fetch("/api/settings/preview", { method: "POST", headers, body: await file.arrayBuffer() });
    if (!res.ok) throw new Error(`Preview failed: ${res.status}`);
    return res.json();
  },
  importSettings: async (file: File): Promise<{ status: string; rebooting: boolean }> => {
    const headers: Record<string, string> = {};
    const token = getToken();
    if (token) headers["Authorization"] = `Bearer ${token}`;
    const res = await fetch("/api/settings/import", { method: "POST", headers, body: await file.arrayBuffer() });
    if (!res.ok) throw new Error(`Import failed: ${res.status}`);
    return res.json();
  },
};

export interface AuthStatus {
  enabled: boolean;
  authenticated: boolean;
}

export interface SettingsPreview {
  hostname: string | null;
  wifi_configured: boolean;
  ssh_keys_present: boolean;
  has_auth: boolean;
  files: string[];
}

export interface NowPlaying {
  playing: boolean;
  title: string;
  artist: string;
  album: string;
  cover_url: string;
  duration_ms: number;
  position_ms: number;
  seekable: boolean;
  can_next: boolean;
  can_prev: boolean;
  volume: number;
  muted: boolean;
}
