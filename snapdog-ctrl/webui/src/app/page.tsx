"use client";

import { useState, useEffect, useCallback, useId, useRef, useMemo } from "react";
import { useTranslations, useLocale } from "next-intl";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Skeleton } from "@/components/ui/skeleton";
import { Select } from "@/components/ui/select";
import { Switch } from "@/components/ui/switch";
import { AboutButton } from "@/components/AboutButton";
import { MiniPlayer } from "@/components/MiniPlayer";
import {
  api,
  type SystemInfo,
  type WifiNetwork,
  type AudioConfig,
  type ClientConfig,
  type Soundcard,
  type SshConfig,
  type ServerConfig,
  type ServerStatus,
  type TuningConfig,
} from "@/lib/api";
import { useI18n } from "@/i18n/provider";
import { locales, type Locale } from "@/i18n/config";
import { useWebSocket } from "@/hooks/useWebSocket";

type Tab = "dashboard" | "network" | "audio" | "client" | "server" | "ssh" | "update" | "system";

const EMOJI_PRESETS = ["🔊", "🛋️", "🍽️", "🛏️", "🎵", "🏠", "🚿", "📺", "💻", "🎧", "🎶", "🌙", "☀️", "🌿", "🏢", "🎮", "📻", "🎹", "🎸", "🥁", "🎺"];

function EmojiPicker({ value, onChange }: { value: string; onChange: (v: string) => void }) {
  const [open, setOpen] = useState(false);
  const [custom, setCustom] = useState(false);
  const isCustom = !EMOJI_PRESETS.includes(value) && value !== "";

  return (
    <div className="relative">
      <button type="button" onClick={() => setOpen(!open)} className="flex size-8 items-center justify-center rounded-md border border-border text-base hover:bg-muted" aria-label="Pick icon">
        {value || "🔊"}
      </button>
      {open && (
        <div className="absolute left-0 top-9 z-50 rounded-lg border border-border bg-card p-2 shadow-lg">
          <div className="grid grid-cols-7 gap-1">
            {EMOJI_PRESETS.map((e) => (
              <button key={e} type="button" className={`size-7 rounded text-base hover:bg-muted ${value === e ? "bg-primary/20" : ""}`} onClick={() => { onChange(e); setOpen(false); setCustom(false); }}>{e}</button>
            ))}
            <button type="button" className={`size-7 rounded text-xs hover:bg-muted ${isCustom || custom ? "bg-primary/20" : ""}`} onClick={() => setCustom(true)}>…</button>
          </div>
          {(custom || isCustom) && (
            <input className="mt-2 w-full rounded border border-border px-2 py-1 text-center text-base" value={value} onChange={(e) => onChange(e.target.value)} placeholder="✨" autoFocus />
          )}
        </div>
      )}
    </div>
  );
}

function StatusDot({ connected, label }: { connected: boolean; label: string }) {
  return (
    <span
      role="img"
      aria-label={label}
      className={`inline-block size-2.5 rounded-full ${connected ? "bg-green-500" : "bg-red-500"}`}
    />
  );
}

function Card({ children, title, id }: { children: React.ReactNode; title: string; id?: string }) {
  return (
    <section aria-labelledby={id} className="rounded-xl border border-border bg-card p-5 shadow-sm">
      <h2
        id={id}
        className="mb-4 text-sm font-semibold uppercase tracking-wide text-muted-foreground"
      >
        {title}
      </h2>
      {children}
    </section>
  );
}

function Field({ label, htmlFor, children }: { label: string; htmlFor?: string; children: React.ReactNode }) {
  const generatedId = useId();
  const id = htmlFor ?? generatedId;
  return (
    <div className="flex flex-col gap-1.5">
      <label htmlFor={id} className="text-sm text-muted-foreground">
        {label}
      </label>
      {children}
    </div>
  );
}

const DEFAULT_SERVER_CONFIG: ServerConfig = {
  name: "SnapDog",
  http: { api_keys: [] },
  audio: {
    sample_rate: 48000,
    bit_depth: 32,
    channels: 2,
    source_conflict: "last_wins",
    zone_switch_fade_ms: 300,
    source_switch_fade_ms: 300,
  },
  snapcast: {
    streaming_port: 1704,
    codec: "f32lz4",
    encryption_psk: null,
    group_volume_mode: "relative",
    unknown_clients: "accept",
    default_zone: "",
    mdns_name: "SnapDog",
    advertise_snapcast: false,
  },
  subsonic: null,
  spotify: null,
  airplay: null,
  mqtt: null,
  knx: null,
  zones: [{ name: "Default", icon: "🔊", knx: null }],
  clients: [],
  radio: [],
  system: { log_level: "info" },
};

type ZoneKnxKey = Extract<keyof NonNullable<ServerConfig["zones"][number]["knx"]>, string>;
type ClientKnxKey = Extract<keyof NonNullable<ServerConfig["clients"][number]["knx"]>, string>;

type KnxField<Key extends string = string> = {
  key: Key;
  label: string;
  dpt: string;
  direction: string;
};

const ZONE_KNX_FIELDS = [
  { key: "play", label: "Play", dpt: "1.001", direction: "→ KNX" },
  { key: "pause", label: "Pause", dpt: "1.001", direction: "→ KNX" },
  { key: "stop", label: "Stop", dpt: "1.001", direction: "→ KNX" },
  { key: "track_next", label: "Next Track", dpt: "1.001", direction: "→ KNX" },
  { key: "track_previous", label: "Previous Track", dpt: "1.001", direction: "→ KNX" },
  { key: "control_status", label: "Playback Status", dpt: "1.001", direction: "← KNX" },
  { key: "volume", label: "Volume", dpt: "5.001", direction: "↔ KNX" },
  { key: "volume_status", label: "Volume Status", dpt: "5.001", direction: "→ KNX" },
  { key: "volume_dim", label: "Volume Dim", dpt: "3.007", direction: "← KNX" },
  { key: "mute", label: "Mute", dpt: "1.001", direction: "↔ KNX" },
  { key: "mute_status", label: "Mute Status", dpt: "1.001", direction: "→ KNX" },
  { key: "mute_toggle", label: "Mute Toggle", dpt: "1.001", direction: "← KNX" },
  { key: "track_title_status", label: "Track Title", dpt: "16.001", direction: "→ KNX" },
  { key: "track_artist_status", label: "Track Artist", dpt: "16.001", direction: "→ KNX" },
  { key: "track_album_status", label: "Track Album", dpt: "16.001", direction: "→ KNX" },
  { key: "track_progress_status", label: "Track Progress", dpt: "5.001", direction: "→ KNX" },
  { key: "track_playing_status", label: "Track Playing", dpt: "1.001", direction: "→ KNX" },
  { key: "track_repeat", label: "Track Repeat", dpt: "1.001", direction: "← KNX" },
  { key: "track_repeat_status", label: "Track Repeat Status", dpt: "1.001", direction: "→ KNX" },
  { key: "track_repeat_toggle", label: "Track Repeat Toggle", dpt: "1.001", direction: "← KNX" },
  { key: "playlist", label: "Playlist", dpt: "5.010", direction: "← KNX" },
  { key: "playlist_status", label: "Playlist Status", dpt: "5.010", direction: "→ KNX" },
  { key: "playlist_next", label: "Playlist Next", dpt: "1.001", direction: "← KNX" },
  { key: "playlist_previous", label: "Playlist Previous", dpt: "1.001", direction: "← KNX" },
  { key: "shuffle", label: "Shuffle", dpt: "1.001", direction: "← KNX" },
  { key: "shuffle_status", label: "Shuffle Status", dpt: "1.001", direction: "→ KNX" },
  { key: "shuffle_toggle", label: "Shuffle Toggle", dpt: "1.001", direction: "← KNX" },
  { key: "repeat", label: "Playlist Repeat", dpt: "1.001", direction: "← KNX" },
  { key: "repeat_status", label: "Playlist Repeat Status", dpt: "1.001", direction: "→ KNX" },
  { key: "repeat_toggle", label: "Playlist Repeat Toggle", dpt: "1.001", direction: "← KNX" },
  { key: "presence", label: "Presence", dpt: "1.001", direction: "← KNX" },
  { key: "presence_enable", label: "Presence Enable", dpt: "1.001", direction: "← KNX" },
  { key: "presence_enable_status", label: "Presence Enable Status", dpt: "1.001", direction: "→ KNX" },
  { key: "presence_timeout", label: "Presence Timeout", dpt: "7.005", direction: "← KNX" },
  { key: "presence_timeout_status", label: "Presence Timeout Status", dpt: "7.005", direction: "→ KNX" },
  { key: "presence_timer_status", label: "Presence Timer", dpt: "1.001", direction: "→ KNX" },
  { key: "presence_source_override", label: "Presence Source Override", dpt: "1.001", direction: "← KNX" },
] as const satisfies readonly KnxField<ZoneKnxKey>[];

const CLIENT_KNX_FIELDS = [
  { key: "volume", label: "Volume", dpt: "5.001", direction: "↔ KNX" },
  { key: "volume_status", label: "Volume Status", dpt: "5.001", direction: "→ KNX" },
  { key: "volume_dim", label: "Volume Dim", dpt: "3.007", direction: "← KNX" },
  { key: "mute", label: "Mute", dpt: "1.001", direction: "↔ KNX" },
  { key: "mute_status", label: "Mute Status", dpt: "1.001", direction: "→ KNX" },
  { key: "mute_toggle", label: "Mute Toggle", dpt: "1.001", direction: "← KNX" },
  { key: "latency", label: "Latency", dpt: "7.005", direction: "← KNX" },
  { key: "latency_status", label: "Latency Status", dpt: "7.005", direction: "→ KNX" },
  { key: "zone", label: "Zone", dpt: "5.010", direction: "← KNX" },
  { key: "zone_status", label: "Zone Status", dpt: "5.010", direction: "→ KNX" },
  { key: "connected_status", label: "Connected Status", dpt: "1.001", direction: "→ KNX" },
] as const satisfies readonly KnxField<ClientKnxKey>[];

function isValidKnxGroupAddress(value: string | null | undefined) {
  const trimmed = value?.trim() ?? "";
  if (!trimmed) return true;
  const parts = trimmed.split("/");
  if (parts.length !== 3) return false;
  const limits = [31, 7, 255];
  return parts.every((part, index) => {
    if (!/^\d+$/.test(part)) return false;
    const numeric = Number(part);
    return Number.isInteger(numeric) && numeric >= 0 && numeric <= limits[index];
  });
}

function normalizeKnxValue(value: string) {
  const trimmed = value.trim();
  return trimmed === "" ? null : trimmed;
}

function compactKnxValues<T extends object>(values: T) {
  const entries = Object.entries(values as Record<string, string | null | undefined>)
    .map(([key, value]) => [key, typeof value === "string" ? value.trim() : ""] as const)
    .filter(([, value]) => value !== "");
  return entries.length > 0 ? Object.fromEntries(entries) as T : null;
}

type ServerTranslator = (key: string, values?: Record<string, string | number>) => string;

function collectServerValidationErrors(config: ServerConfig, t: ServerTranslator) {
  const errors: string[] = [];
  if (config.knx?.role === "client" && !(config.knx.url ?? "").trim()) {
    errors.push(t("knxGatewayRequired"));
  }

  const addKnxErrors = (
    target: string,
    fields: readonly KnxField[],
    values: object | null
  ) => {
    if (!values) return;
    const valueBag = values as Record<string, string | null | undefined>;
    for (const field of fields) {
      const value = valueBag[field.key];
      if (value && !isValidKnxGroupAddress(value)) {
        errors.push(t("knxInvalidGaFor", { target, field: field.label }));
      }
    }
  };

  for (const [index, zone] of config.zones.entries()) {
    addKnxErrors(zone.name || `${t("zone")} ${index + 1}`, ZONE_KNX_FIELDS, zone.knx);
  }
  for (const [index, client] of config.clients.entries()) {
    addKnxErrors(client.name || `${t("clientName")} ${index + 1}`, CLIENT_KNX_FIELDS, client.knx);
  }

  return errors;
}

// ── Dashboard Tab ─────────────────────────────────────────────

function DashboardTab() {
  const t = useTranslations("dashboard");
  const [info, setInfo] = useState<SystemInfo | null>(null);
  const [wifi, setWifi] = useState<import("@/lib/api").WifiStatus | null>(null);
  const [eth, setEth] = useState<import("@/lib/api").EthernetStatus | null>(null);
  const cardId = useId();

  useEffect(() => {
    api.getSystem().then(setInfo).catch(() => {});
    api.getWifi().then(setWifi).catch(() => {});
    api.getEthernet().then(setEth).catch(() => {});
  }, []);

  if (!info) return <Skeleton className="h-40 w-full" aria-label={t("loading")} />;

  const uptimeHours = Math.floor(info.uptime_seconds / 3600);
  const uptimeMinutes = Math.floor((info.uptime_seconds % 3600) / 60);

  return (
    <Card title={t("title")} id={cardId}>
      <dl className="grid grid-cols-[auto_minmax(0,1fr)] gap-x-4 gap-y-3 text-sm">
        <dt className="text-muted-foreground">{t("hostname")}</dt>
        <dd className="font-medium">{info.hostname || "—"}</dd>
        <dt className="text-muted-foreground">{t("version")}</dt>
        <dd className="min-w-0 font-mono text-xs">
          <span className="block">{info.version || "—"}</span>
          <div className="mt-1 max-w-full break-all text-[10px] leading-relaxed text-muted-foreground">
            Client {info.components.client} · Server {info.components.server} · Ctrl {info.components.ctrl} · Kernel {info.components.kernel}
          </div>
        </dd>
        <dt className="text-muted-foreground">{t("network")}</dt>
        <dd className="space-y-1">
          {wifi && (
            <div className="flex items-center gap-2">
              <StatusDot connected={wifi.connected} label={wifi.connected ? t("wifiConnected") : t("wifiDisconnected")} />
              <span>{wifi.connected ? `WiFi (${wifi.ip})` : t("wifiDisconnected")}</span>
            </div>
          )}
          {eth && (
            <div className="flex items-center gap-2">
              <StatusDot connected={eth.connected} label={eth.connected ? t("ethConnected") : t("ethDisconnected")} />
              <span>{eth.connected ? `Ethernet (${eth.ip})` : t("ethDisconnected")}</span>
            </div>
          )}
        </dd>
        <dt className="text-muted-foreground">{t("uptime")}</dt>
        <dd>{uptimeHours}h {uptimeMinutes}m</dd>
        <dt className="text-muted-foreground">{t("boardModel")}</dt>
        <dd>{info.board_model || "—"}</dd>
      </dl>
    </Card>
  );
}

// ── Network Tab ───────────────────────────────────────────────


function NetworkDetails({ ip, subnet, gateway, dns }: { ip: string; subnet: string; gateway: string; dns: string }) {
  const t = useTranslations("network");
  if (!ip) return null;
  return (
    <dl className="mt-3 grid grid-cols-[auto_1fr] gap-x-3 gap-y-1 rounded-lg bg-muted/50 p-3 text-xs">
      <dt className="text-muted-foreground">{t("ipAddress")}</dt>
      <dd className="font-mono">{ip}</dd>
      <dt className="text-muted-foreground">{t("subnet")}</dt>
      <dd className="font-mono">{subnet}</dd>
      <dt className="text-muted-foreground">{t("gateway")}</dt>
      <dd className="font-mono">{gateway}</dd>
      <dt className="text-muted-foreground">{t("dns")}</dt>
      <dd className="font-mono">{dns}</dd>
    </dl>
  );
}

// Host the setup access point answers on. When the browser is talking to the
// device over this address we're on the temporary AP, which disappears the
// moment WiFi associates — so we can't poll and must instruct the user instead.
const AP_SETUP_HOST = "10.11.12.13";
// Sentinel selection for the "Other / hidden network…" row.
const MANUAL_SSID = "__manual__";

/** Map a dBm reading to a 1–4 bar strength. */
function wifiSignalLevel(dbm: number): 1 | 2 | 3 | 4 {
  if (dbm >= -55) return 4;
  if (dbm >= -66) return 3; // -66..-56
  if (dbm >= -77) return 2; // -77..-67
  return 1; // < -77
}

function SignalBars({ dbm, label }: { dbm: number; label: string }) {
  const level = wifiSignalLevel(dbm);
  return (
    <span
      className="inline-flex shrink-0 items-end gap-0.5"
      title={`${dbm} dBm`}
      role="img"
      aria-label={label}
    >
      {([1, 2, 3, 4] as const).map((bar) => (
        <span
          key={bar}
          aria-hidden="true"
          className={`w-1 rounded-sm ${bar <= level ? "bg-foreground" : "bg-muted-foreground/25"}`}
          style={{ height: `${2 + bar * 3}px` }}
        />
      ))}
    </span>
  );
}

function SecurityIcon({ security, openLabel, securedLabel }: { security: string; openLabel: string; securedLabel: string }) {
  const isOpen = security === "open";
  const title = isOpen ? openLabel : `${securedLabel} · ${security.toUpperCase()}`;
  return (
    <svg
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      className={`size-3.5 shrink-0 ${isOpen ? "text-muted-foreground/40" : "text-muted-foreground"}`}
      role="img"
      aria-label={title}
    >
      <title>{title}</title>
      <rect x="4.5" y="10.5" width="15" height="9.5" rx="2" strokeWidth={1.6} />
      {isOpen ? (
        <path d="M8 10.5V7a4 4 0 0 1 7.8-1.3" strokeWidth={1.6} strokeLinecap="round" />
      ) : (
        <path d="M8 10.5V7a4 4 0 1 1 8 0v3.5" strokeWidth={1.6} strokeLinecap="round" />
      )}
    </svg>
  );
}

/** Small inline status banner used for scan/connect feedback. */
function Banner({ tone, busy, children }: { tone: "info" | "warn" | "success" | "muted"; busy?: boolean; children: React.ReactNode }) {
  const tones = {
    info: "bg-blue-500/10 text-blue-800 dark:text-blue-300",
    warn: "bg-amber-500/10 text-amber-800 dark:text-amber-300",
    success: "bg-green-500/10 text-green-700 dark:text-green-300",
    muted: "bg-muted/60 text-muted-foreground",
  } as const;
  return (
    <div className={`flex items-center gap-2 rounded-lg px-3 py-2 text-xs ${tones[tone]}`} role="status">
      {busy && (
        <svg className="size-3.5 shrink-0 animate-spin" fill="none" viewBox="0 0 24 24" aria-hidden="true">
          <circle className="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" strokeWidth="4" />
          <path className="opacity-75" fill="currentColor" d="M4 12a8 8 0 0 1 8-8V0C5.373 0 0 5.373 0 12h4z" />
        </svg>
      )}
      <span>{children}</span>
    </div>
  );
}

function NetworkTab() {
  const t = useTranslations("network");
  const [networks, setNetworks] = useState<WifiNetwork[]>([]);
  const [scanStatus, setScanStatus] = useState<"ok" | "unavailable_ap_mode" | "error" | null>(null);
  const [apActive, setApActive] = useState(false);
  const [scanning, setScanning] = useState(true);
  // Selection: a scanned SSID, MANUAL_SSID for "other / hidden", or "" (none yet).
  const [selection, setSelection] = useState("");
  // Latest scan + WiFi status, mirrored into refs so the two independent async
  // loads can pre-select the connected network regardless of which lands first.
  const networksRef = useRef<WifiNetwork[]>([]);
  const wifiStatusRef = useRef<import("@/lib/api").WifiStatus | null>(null);
  const [manualSsid, setManualSsid] = useState("");
  const [password, setPassword] = useState("");
  const [wifiMode, setWifiMode] = useState<"dhcp" | "static">("dhcp");
  const [wifiIp, setWifiIp] = useState("");
  const [wifiSubnet, setWifiSubnet] = useState("255.255.255.0");
  const [wifiGateway, setWifiGateway] = useState("");
  const [wifiDns, setWifiDns] = useState("");
  const [wifiStatus, setWifiStatus] = useState<import("@/lib/api").WifiStatus | null>(null);
  // Connect feedback state machine.
  const [connectState, setConnectState] = useState<
    "idle" | "submitting" | "connecting" | "success" | "auth_failed" | "timeout" | "ap_redirect" | "error"
  >("idle");
  const [connectError, setConnectError] = useState("");
  const [connectResult, setConnectResult] = useState<{ ssid: string; ip: string } | null>(null);
  const pollRef = useRef<ReturnType<typeof setInterval> | null>(null);
  const [ethStatus, setEthStatus] = useState<import("@/lib/api").EthernetStatus | null>(null);
  const [ethMode, setEthMode] = useState<"dhcp" | "static">("dhcp");
  const [ethIp, setEthIp] = useState("");
  const [ethSubnet, setEthSubnet] = useState("255.255.255.0");
  const [ethGateway, setEthGateway] = useState("");
  const [ethDns, setEthDns] = useState("");
  const ssidId = useId();
  const passwordId = useId();
  const wifiModeId = useId();
  const wifiIpId = useId();
  const wifiSubnetId = useId();
  const wifiGatewayId = useId();
  const wifiDnsId = useId();
  const ethModeId = useId();
  const ethIpId = useId();
  const ethSubnetId = useId();
  const ethGatewayId = useId();
  const ethDnsId = useId();
  const wifiCardId = useId();
  const ethCardId = useId();

  const manualSelected = selection === MANUAL_SSID;
  const selectedNetwork = manualSelected ? null : networks.find((n) => n.ssid === selection) ?? null;
  const effectiveSsid = manualSelected ? manualSsid.trim() : selectedNetwork?.ssid ?? "";
  // Only a *scanned* open network lets us safely drop the password field; for a
  // hidden/manual network the security is unknown, so keep it (blank means open).
  const passwordHidden = selectedNetwork?.security === "open";
  const effectivePassword = passwordHidden ? "" : password;
  // Open networks need no key; secured ones require a valid WPA passphrase (8–63).
  // Gates the connect button so a blank/short password can't be submitted (the
  // backend would 400 it, and a psk="" config would otherwise break the supplicant).
  const passwordValid = passwordHidden || (password.length >= 8 && password.length <= 63);

  // Cancel any in-flight connect poll and reset feedback (on any edit/selection).
  const resetConnect = () => {
    if (pollRef.current) { clearInterval(pollRef.current); pollRef.current = null; }
    setConnectState("idle");
  };

  const applyScan = useCallback((r: import("@/lib/api").WifiScanResult) => {
    networksRef.current = r.networks;
    setNetworks(r.networks);
    setScanStatus(r.status);
    setApActive(r.ap_active);
    setSelection((prev) => {
      // Preserve an explicit choice (manual, or a still-visible pick).
      if (prev === MANUAL_SSID || r.networks.some((n) => n.ssid === prev)) return prev;
      // Otherwise pre-select the currently-connected network if it's in range
      // (the WiFi status may or may not have loaded yet — getWifi's callback
      // retries this once it has).
      const w = wifiStatusRef.current;
      if (w?.connected && w.ssid && r.networks.some((n) => n.ssid === w.ssid)) return w.ssid;
      // Keep the manual field reachable immediately when there's nothing to pick.
      return r.networks.length === 0 ? MANUAL_SSID : "";
    });
  }, []);

  const scan = useCallback(() => {
    setScanning(true);
    api.scanWifi().then(applyScan).catch(() => setScanStatus("error")).finally(() => setScanning(false));
  }, [applyScan]);

  // Pre-select the network the device is CONNECTED to once BOTH the scan and the
  // WiFi status are loaded and it's in range — but only when nothing is selected
  // yet (never overrides the user's pick). Done from the load callbacks (not a
  // reactive effect) to avoid the set-state-in-effect cascading-render lint; the
  // refs bridge the two independent async loads regardless of which lands first.
  const preselectConnected = useCallback(() => {
    const nets = networksRef.current;
    const w = wifiStatusRef.current;
    if (!w?.connected || !w.ssid || !nets.some((n) => n.ssid === w.ssid)) return;
    setSelection((prev) => (prev === "" ? w.ssid : prev));
  }, []);

  useEffect(() => {
    api.scanWifi().then(applyScan).catch(() => setScanStatus("error")).finally(() => setScanning(false));
    api.getWifi().then((w) => {
      wifiStatusRef.current = w;
      setWifiStatus(w);
      if (w.mode) setWifiMode(w.mode);
      // Pre-load the static fields (like Ethernet does) so re-saving while on
      // static mode doesn't clobber the persisted IP/gateway/DNS with blanks.
      if (w.ip) setWifiIp(w.ip);
      if (w.subnet) setWifiSubnet(w.subnet);
      if (w.gateway) setWifiGateway(w.gateway);
      if (w.dns) setWifiDns(w.dns);
      preselectConnected(); // pre-select if the scan already populated the list
    }).catch(() => {});
    api.getEthernet().then((e) => {
      setEthStatus(e);
      if (e.mode) setEthMode(e.mode as "dhcp" | "static");
      if (e.ip) setEthIp(e.ip);
      if (e.subnet) setEthSubnet(e.subnet);
      if (e.gateway) setEthGateway(e.gateway);
      if (e.dns) setEthDns(e.dns);
    }).catch(() => {});
    return () => { if (pollRef.current) clearInterval(pollRef.current); };
  }, [applyScan, preselectConnected]);

  // Submit → (202) connecting → poll GET /wifi ≤30s → success | auth_failed | timeout.
  // Special case: if we reached the device over the setup AP, that AP tears down
  // as WiFi comes up, so polling is impossible — instruct the user to reconnect.
  const connect = async () => {
    if (!effectiveSsid || connectState === "submitting" || connectState === "connecting") return;
    if (pollRef.current) { clearInterval(pollRef.current); pollRef.current = null; }
    setConnectError("");
    setConnectResult(null);
    setConnectState("submitting");
    try {
      await api.setWifi({
        ssid: effectiveSsid,
        password: effectivePassword,
        mode: wifiMode,
        ...(wifiMode === "static" ? { ip: wifiIp, subnet: wifiSubnet, gateway: wifiGateway, dns: wifiDns } : {}),
      });
    } catch (e) {
      setConnectError(e instanceof Error ? e.message : String(e));
      setConnectState("error");
      return;
    }
    if (typeof window !== "undefined" && window.location.hostname === AP_SETUP_HOST) {
      setConnectState("ap_redirect");
      return;
    }
    setConnectState("connecting");
    const startedAt = Date.now();
    const poll = async () => {
      try {
        const w = await api.getWifi();
        setWifiStatus(w);
        if (w.state === "connected" && w.ip) {
          if (pollRef.current) { clearInterval(pollRef.current); pollRef.current = null; }
          setConnectResult({ ssid: w.ssid || effectiveSsid, ip: w.ip });
          setConnectState("success");
          return;
        }
        if (w.state === "auth_failed") {
          if (pollRef.current) { clearInterval(pollRef.current); pollRef.current = null; }
          setConnectState("auth_failed");
          return;
        }
      } catch {
        // Transient drop while the radio switches networks — keep polling.
      }
      if (Date.now() - startedAt >= 30000) {
        if (pollRef.current) { clearInterval(pollRef.current); pollRef.current = null; }
        setConnectState("timeout");
      }
    };
    pollRef.current = setInterval(() => { void poll(); }, 2000);
  };

  return (
    <div className="space-y-5">
      <Card title={t("wifi")} id={wifiCardId}>
        <div className="space-y-3">
          {wifiStatus?.connected && (
            <div className="flex flex-wrap items-center gap-2 text-sm">
              <StatusDot connected label={t("connectedTo")} />
              <span className="text-muted-foreground">{t("connectedTo")}</span>
              <span className="font-medium">{wifiStatus.ssid}</span>
              <span className="text-xs text-muted-foreground">({wifiStatus.signal} dBm)</span>
            </div>
          )}
          {wifiStatus?.connected && (
            <NetworkDetails ip={wifiStatus.ip} subnet={wifiStatus.subnet} gateway={wifiStatus.gateway} dns={wifiStatus.dns} />
          )}

          <div className="flex items-center justify-between">
            <span className="text-sm text-muted-foreground">{t("availableNetworks")}</span>
            <Button variant="outline" size="sm" onClick={scan} disabled={scanning} aria-busy={scanning}>
              {scanning ? t("scanning") : t("rescan")}
            </Button>
          </div>

          {!scanning && networks.length === 0 && (
            scanStatus === "unavailable_ap_mode" || apActive ? (
              <Banner tone="info">{t("bannerApMode")}</Banner>
            ) : scanStatus === "error" ? (
              <Banner tone="warn">{t("bannerScanError")}</Banner>
            ) : (
              <Banner tone="muted">{t("bannerNoneFound")}</Banner>
            )
          )}

          <ul role="radiogroup" aria-label={t("availableNetworks")} className="max-h-56 space-y-1 overflow-y-auto">
            {networks.map((n) => {
              const selected = !manualSelected && selection === n.ssid;
              return (
                <li key={n.ssid}>
                  <button
                    type="button"
                    role="radio"
                    aria-checked={selected}
                    onClick={() => { setSelection(n.ssid); resetConnect(); }}
                    className={`flex w-full items-center gap-3 rounded-lg border px-3 py-2 text-left text-sm transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring ${selected ? "border-primary bg-primary/5" : "border-border hover:bg-muted"}`}
                  >
                    <SignalBars dbm={n.signal} label={t("signalAria", { ssid: n.ssid, dbm: n.signal })} />
                    <span className="flex-1 truncate">{n.ssid}</span>
                    <SecurityIcon security={n.security} openLabel={t("openNetwork")} securedLabel={t("secured")} />
                  </button>
                </li>
              );
            })}
            <li>
              <button
                type="button"
                role="radio"
                aria-checked={manualSelected}
                onClick={() => { setSelection(MANUAL_SSID); resetConnect(); }}
                className={`flex w-full items-center gap-3 rounded-lg border px-3 py-2 text-left text-sm transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring ${manualSelected ? "border-primary bg-primary/5" : "border-border hover:bg-muted"}`}
              >
                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" className="size-3.5 shrink-0 text-muted-foreground" aria-hidden="true">
                  <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={1.6} d="M12 5v14M5 12h14" />
                </svg>
                <span className="flex-1">{t("otherNetwork")}</span>
              </button>
            </li>
          </ul>

          {manualSelected && (
            <Field label={t("ssid")} htmlFor={ssidId}>
              <Input
                id={ssidId}
                value={manualSsid}
                onChange={(e) => { setManualSsid(e.target.value); resetConnect(); }}
                autoComplete="off"
                autoCapitalize="none"
                spellCheck={false}
                placeholder={t("ssid")}
              />
              <p className="text-xs text-muted-foreground">{t("hiddenSsidHint")}</p>
            </Field>
          )}

          {!passwordHidden && (
            <Field label={t("password")} htmlFor={passwordId}>
              <Input id={passwordId} type="password" value={password} onChange={(e) => { setPassword(e.target.value); resetConnect(); }} autoComplete="current-password" />
            </Field>
          )}

          <Field label={t("mode")} htmlFor={wifiModeId}>
            <Select id={wifiModeId} value={wifiMode} onChange={(e) => setWifiMode(e.target.value as "dhcp" | "static")}>
              <option value="dhcp">DHCP</option>
              <option value="static">{t("static")}</option>
            </Select>
          </Field>
          {wifiMode === "static" && (
            <>
              <Field label={t("ipAddress")} htmlFor={wifiIpId}>
                <Input id={wifiIpId} value={wifiIp} onChange={(e) => setWifiIp(e.target.value)} inputMode="decimal" placeholder="192.168.1.100" />
              </Field>
              <Field label={t("subnet")} htmlFor={wifiSubnetId}>
                <Input id={wifiSubnetId} value={wifiSubnet} onChange={(e) => setWifiSubnet(e.target.value)} inputMode="decimal" placeholder="255.255.255.0" />
              </Field>
              <Field label={t("gateway")} htmlFor={wifiGatewayId}>
                <Input id={wifiGatewayId} value={wifiGateway} onChange={(e) => setWifiGateway(e.target.value)} inputMode="decimal" placeholder="192.168.1.1" />
              </Field>
              <Field label={t("dns")} htmlFor={wifiDnsId}>
                <Input id={wifiDnsId} value={wifiDns} onChange={(e) => setWifiDns(e.target.value)} inputMode="decimal" placeholder="1.1.1.1" />
              </Field>
            </>
          )}

          {connectState === "connecting" && <Banner tone="info" busy>{t("connecting")}</Banner>}
          {connectState === "success" && connectResult && (
            <Banner tone="success">{t("connectSuccess", { ssid: connectResult.ssid, ip: connectResult.ip })}</Banner>
          )}
          {connectState === "auth_failed" && <Banner tone="warn">{t("authFailed")}</Banner>}
          {connectState === "timeout" && <Banner tone="warn">{t("connectTimeout")}</Banner>}
          {connectState === "error" && <Banner tone="warn">{t("connectError", { reason: connectError })}</Banner>}
          {connectState === "ap_redirect" && (
            <div className="rounded-xl border border-primary/30 bg-primary/5 p-4 text-sm">
              <p className="font-semibold">{t("apRedirectTitle")}</p>
              <p className="mt-1 text-muted-foreground">{t("apRedirectBody")}</p>
            </div>
          )}

          {connectState !== "ap_redirect" && (
            <Button
              size="sm"
              onClick={() => void connect()}
              disabled={!effectiveSsid || !passwordValid || connectState === "submitting" || connectState === "connecting"}
              aria-busy={connectState === "submitting" || connectState === "connecting"}
            >
              {connectState === "submitting" ? t("submitting") : t("connect")}
            </Button>
          )}
          {wifiStatus?.connected && connectState !== "ap_redirect" && (
            <Button variant="outline" size="sm" onClick={() => { api.disconnectWifi().catch(() => {}).finally(() => { api.getWifi().then(setWifiStatus).catch(() => {}); }); }}>
              {t("disconnect")}
            </Button>
          )}
        </div>
      </Card>
      <Card title={t("ethernet")} id={ethCardId}>
        <div className="space-y-3">
          {ethStatus?.connected && (
            <div className="flex items-center gap-2 text-sm">
              <StatusDot connected label={t("ethernetConnected")} />
              <span className="font-medium">{t("ethernetConnected")}</span>
            </div>
          )}
          {ethStatus?.connected && (
            <NetworkDetails ip={ethStatus.ip} subnet={ethStatus.subnet} gateway={ethStatus.gateway} dns={ethStatus.dns} />
          )}
          <Field label={t("mode")} htmlFor={ethModeId}>
            <Select id={ethModeId} value={ethMode} onChange={(e) => setEthMode(e.target.value as "dhcp" | "static")}>
              <option value="dhcp">DHCP</option>
              <option value="static">{t("static")}</option>
            </Select>
          </Field>
          {ethMode === "static" && (
            <>
              <Field label={t("ipAddress")} htmlFor={ethIpId}>
                <Input id={ethIpId} value={ethIp} onChange={(e) => setEthIp(e.target.value)} inputMode="decimal" placeholder="192.168.1.100" />
              </Field>
              <Field label={t("subnet")} htmlFor={ethSubnetId}>
                <Input id={ethSubnetId} value={ethSubnet} onChange={(e) => setEthSubnet(e.target.value)} inputMode="decimal" placeholder="255.255.255.0" />
              </Field>
              <Field label={t("gateway")} htmlFor={ethGatewayId}>
                <Input id={ethGatewayId} value={ethGateway} onChange={(e) => setEthGateway(e.target.value)} inputMode="decimal" placeholder="192.168.1.1" />
              </Field>
              <Field label={t("dns")} htmlFor={ethDnsId}>
                <Input id={ethDnsId} value={ethDns} onChange={(e) => setEthDns(e.target.value)} inputMode="decimal" placeholder="1.1.1.1" />
              </Field>
            </>
          )}
          <Button size="sm" onClick={() => api.setEthernet({ mode: ethMode, ...(ethMode === "static" ? { ip: ethIp, subnet: ethSubnet, gateway: ethGateway, dns: ethDns } : {}) })}>
            {t("save")}
          </Button>
        </div>
      </Card>
      <SoftApCard />
    </div>
  );
}

// Common Wi-Fi regulatory domains (ISO-3166 alpha-2). Region names are localised
// at render time via Intl.DisplayNames, so this stays a plain code list.
const WIFI_COUNTRIES = [
  "AR", "AT", "AU", "BE", "BG", "BR", "CA", "CH", "CL", "CN", "CZ", "DE", "DK",
  "EE", "ES", "FI", "FR", "GB", "GR", "HK", "HR", "HU", "IE", "IL", "IN", "IS",
  "IT", "JP", "KR", "LT", "LU", "LV", "MX", "MY", "NL", "NO", "NZ", "PH", "PL",
  "PT", "RO", "RS", "RU", "SE", "SG", "SI", "SK", "TH", "TR", "TW", "UA", "US",
  "VN", "ZA",
];

function SoftApCard() {
  const t = useTranslations("network.softap");
  const locale = useLocale();
  const id = useId();
  const [view, setView] = useState<import("@/lib/api").SoftApView | null>(null);
  const [enabled, setEnabled] = useState(true);
  const [password, setPassword] = useState("");
  const [country, setCountry] = useState("DE");
  const [status, setStatus] = useState<"idle" | "saved">("idle");
  const [error, setError] = useState("");
  const [saving, setSaving] = useState(false);

  const countryOptions = useMemo(() => {
    let dn: Intl.DisplayNames | undefined;
    try {
      dn = new Intl.DisplayNames([locale], { type: "region" });
    } catch {
      dn = undefined;
    }
    const list = WIFI_COUNTRIES.map((code) => ({ code, name: dn?.of(code) ?? code }));
    // Keep the persisted value selectable even if it's outside the curated list.
    if (country && !list.some((c) => c.code === country)) {
      list.push({ code: country, name: dn?.of(country) ?? country });
    }
    return list.sort((a, b) => a.name.localeCompare(b.name, locale));
  }, [locale, country]);

  useEffect(() => {
    api.getSoftap().then((v) => {
      setView(v);
      setEnabled(v.enabled);
      // Passphrase is only exposed while the AP is up; otherwise start empty.
      setPassword(v.password ?? "");
      setCountry(v.country || "DE");
    }).catch(() => {});
  }, []);

  const persist = async (next: { enabled: boolean; password: string; country: string }): Promise<boolean> => {
    setError("");
    setSaving(true);
    try {
      await api.setSoftap(next);
      setStatus("saved");
      setTimeout(() => setStatus("idle"), 2000);
      return true;
    } catch (e) {
      // 409 (lockout guard) or 400 (password too short) — surface the reason.
      setError(e instanceof Error ? e.message : String(e));
      return false;
    } finally {
      setSaving(false);
    }
  };

  const onToggle = async (next: boolean) => {
    setEnabled(next);
    // Enabling with an unsaved short password: let them fill it in before saving.
    if (next && password.length < 8) return;
    const ok = await persist({ enabled: next, password, country });
    if (!ok) setEnabled(!next); // revert so the switch reflects the real state
  };

  const saveField = () => {
    if (password.length < 8) return;
    void persist({ enabled, password, country });
  };

  return (
    <Card title={t("title")} id={id}>
      <div className="space-y-3">
        <p className="text-xs text-muted-foreground">{t("description")}</p>

        {view && (
          <dl className="grid grid-cols-[auto_1fr] gap-x-3 gap-y-1 rounded-lg bg-muted/50 p-3 text-xs">
            <dt className="text-muted-foreground">{t("networkName")}</dt>
            <dd className="font-mono break-all">{view.ssid}</dd>
            {view.password && (
              <>
                <dt className="text-muted-foreground">{t("passphrase")}</dt>
                <dd className="font-mono break-all">{view.password}</dd>
              </>
            )}
          </dl>
        )}

        <div className="flex items-center justify-between">
          <span className="text-sm">{t("enable")}</span>
          <Switch checked={enabled} onCheckedChange={(v) => void onToggle(v)} disabled={saving} />
        </div>

        <div className="flex flex-col gap-1.5">
          <label htmlFor={`${id}-pw`} className="text-sm text-muted-foreground">{t("password")}</label>
          <Input
            id={`${id}-pw`}
            type="text"
            value={password}
            onChange={(e) => setPassword(e.target.value)}
            onBlur={saveField}
            minLength={8}
            autoComplete="off"
            aria-invalid={password.length > 0 && password.length < 8}
          />
          {password.length < 8 && <p className="text-xs text-destructive">{t("passwordHint")}</p>}
        </div>

        <div className="flex flex-col gap-1.5">
          <label htmlFor={`${id}-cc`} className="text-sm text-muted-foreground">{t("country")}</label>
          <Select
            id={`${id}-cc`}
            value={country}
            onChange={(e) => {
              const cc = e.target.value;
              setCountry(cc);
              // Persist immediately, but only when the AP password is valid — an
              // empty/short password would be rejected by the backend.
              if (password.length >= 8) void persist({ enabled, password, country: cc });
            }}
          >
            {countryOptions.map((c) => (
              <option key={c.code} value={c.code}>{c.name} ({c.code})</option>
            ))}
          </Select>
          <p className="text-xs text-muted-foreground">{t("countryHint")}</p>
        </div>

        {error && (
          <div className="rounded-lg bg-destructive/10 px-3 py-2 text-xs text-destructive" role="status">
            <p className="font-medium">{t("saveError")}</p>
            <p className="mt-0.5 opacity-90">{error}</p>
          </div>
        )}
        {status === "saved" && !error && <p className="text-xs text-green-600">{t("saved")}</p>}
      </div>
    </Card>
  );
}

function AudioTab() {
  const t = useTranslations("audio");
  const [config, setConfig] = useState<AudioConfig | null>(null);
  const overlayId = useId();
  const cardId = useId();

  useEffect(() => {
    api.getAudio().then(setConfig).catch(() => {});
  }, []);

  useWebSocket("audio_changed", useCallback(() => {
    api.getAudio().then(setConfig).catch(() => {});
  }, []));

  if (!config) return <Skeleton className="h-32 w-full" aria-label={t("loading")} />;

  const detectedName = config.detected_hat
    ? config.available_overlays.find((o) => o.id === config.detected_hat)?.name ?? config.detected_hat
    : null;
  // The "Auto-Detect" overlay row has an empty id. When it's active, surface what
  // detection actually resolved to (the EEPROM HAT if any, else the live ALSA
  // card, else "nothing yet") so the user can see the outcome.
  const isAutoDetect = config.overlay === "";
  const autoResult = detectedName ?? (config.detected_card || null);

  return (
    <Card title={t("title")} id={cardId}>
      <div className="space-y-3">
        {isAutoDetect ? (
          <div className="rounded-lg bg-muted/40 px-3 py-2 text-xs text-muted-foreground" role="status">
            {autoResult ? (
              <span>{t("autoDetectResult")}: <strong className="text-foreground">{autoResult}</strong></span>
            ) : (
              t("autoDetectNone")
            )}
          </div>
        ) : detectedName && config.overlay !== config.detected_hat ? (
          <div className="flex items-center justify-between rounded-lg bg-blue-500/10 px-3 py-2 text-xs text-blue-800 dark:text-blue-300">
            <span>{t("detected")}: <strong>{detectedName}</strong></span>
            <Button size="xs" variant="outline" onClick={() => {
              api.setAudio(config.detected_hat);
              setConfig({ ...config, overlay: config.detected_hat });
            }}>{t("apply")}</Button>
          </div>
        ) : detectedName && config.overlay === config.detected_hat ? (
          <p className="text-xs text-green-600">✓ {t("usingDetected")}: {detectedName}</p>
        ) : null}
        <Field label={t("dacOverlay")} htmlFor={overlayId}>
          <Select
            id={overlayId}
            value={config.overlay}
            onChange={(e) => {
              setConfig({ ...config, overlay: e.target.value });
              api.setAudio(e.target.value);
            }}
          >
            {config.available_overlays.map((o) => (
              <option key={o.id} value={o.id}>{o.name}</option>
            ))}
          </Select>
        </Field>
        {config.detected_card && (
          <Field label={t("detectedCard")}>
            <p className="font-mono text-xs text-foreground">{config.detected_card}</p>
          </Field>
        )}
      </div>
    </Card>
  );
}

// ── Client Tab ────────────────────────────────────────────────

function ClientTab() {
  const t = useTranslations("client");
  const [config, setConfig] = useState<ClientConfig>({ server_url: "", host_id: "", soundcard: "default", mixer: "", latency: 0 });
  const [soundcards, setSoundcards] = useState<Soundcard[]>([]);
  const [manualCustomSoundcard, setManualCustomSoundcard] = useState(false);
  const [servers, setServers] = useState<{ name: string; host: string; port: number }[]>([]);
  const [scanning, setScanning] = useState(true);
  const [manualHost, setManualHost] = useState("");
  const [manualPort, setManualPort] = useState("1704");
  const [saving, setSaving] = useState(false);
  const [clientEnabled, setClientEnabled] = useState(true);
  const [serverRunning, setServerRunning] = useState(false);
  const [connectionMode, setConnectionMode] = useState<"auto" | "manual">("auto");
  const [testStatus, setTestStatus] = useState<"idle" | "testing" | "success" | "failed">("idle");

  // Soundcard picker: a dropdown of detected cards, with a "Custom…" escape.
  // Custom mode auto-engages when a saved value isn't "default" or a listed card.
  const soundcardKnown =
    config.soundcard === "default" || soundcards.some((sc) => sc.device === config.soundcard);
  const soundcardCustom = manualCustomSoundcard || (!soundcardKnown && config.soundcard !== "");

  const hostIdFieldId = useId();
  const soundcardId = useId();
  const mixerId = useId();
  const latencyId = useId();
  const cardId = useId();
  const enableId = useId();

  const scanForServers = useCallback(() => {
    setScanning(true);
    api.scanServers().then((r) => setServers(r.servers)).catch(() => {}).finally(() => setScanning(false));
  }, []);

  useEffect(() => {
    Promise.all([api.getClient(), api.scanServers()])
      .then(([c, r]) => {
        setConfig(c);
        setClientEnabled(c.server_url !== "__disabled__"); // reflect persisted enable state
        if (c.available_soundcards) setSoundcards(c.available_soundcards);
        setServers(r.servers);

        // Smart mode detection
        if (c.server_url && c.server_url !== "__disabled__") {
          const match = c.server_url.match(/^tcp:\/\/(.+):(\d+)$/);
          if (match) {
            const host = match[1];
            const port = match[2];
            setManualHost(host);
            setManualPort(port);
            const isDiscovered = r.servers.some((s) => s.host === host && s.port === Number(port));
            setConnectionMode(isDiscovered ? "auto" : "manual");
          } else {
            setConnectionMode("manual");
          }
        } else {
          setConnectionMode("auto");
        }
      })
      .catch(() => {
        // Resilient fallback in case Promise.all fails
        api.getClient().then((c) => {
          setConfig(c);
          setClientEnabled(c.server_url !== "__disabled__");
          if (c.available_soundcards) setSoundcards(c.available_soundcards);
          setConnectionMode(c.server_url ? "manual" : "auto");
          const match = c.server_url.match(/^tcp:\/\/(.+):(\d+)$/);
          if (match) {
            setManualHost(match[1]);
            setManualPort(match[2]);
          }
        }).catch(() => {});
      })
      .finally(() => setScanning(false));

    api.getServerStatus().then((s) => {
      setServerRunning(s.running);
    }).catch(() => {});
  }, []);

  useWebSocket("client_changed", useCallback(() => {
    api.getClient().then((c) => {
      setConfig(c);
      setClientEnabled(c.server_url !== "__disabled__");
      if (c.available_soundcards) setSoundcards(c.available_soundcards);
    }).catch(() => {});
  }, []));

  const selectServer = (url: string) => {
    setConfig((prev) => ({ ...prev, server_url: url }));
  };

  const handleManualHostChange = (val: string) => {
    setManualHost(val);
    setTestStatus("idle");
  };

  const handleManualPortChange = (val: string) => {
    setManualPort(val);
    setTestStatus("idle");
  };

  const testManualConnection = async () => {
    if (!manualHost) return;
    setTestStatus("testing");
    try {
      const port = Number(manualPort) || 1704;
      const res = await api.testServer(manualHost, port);
      setTestStatus(res.reachable ? "success" : "failed");
    } catch {
      setTestStatus("failed");
    }
  };

  const saveConfig = useCallback(async () => {
    const url = connectionMode === "manual"
      ? (manualHost ? `tcp://${manualHost}:${manualPort}` : "")
      : config.server_url;

    if (url) {
      const host = connectionMode === "manual" ? manualHost : url.replace(/^tcp:\/\//, "").split(":")[0];
      const port = connectionMode === "manual" ? Number(manualPort) : (Number(url.replace(/^tcp:\/\//, "").split(":")[1]) || 1704);
      setSaving(true);
      try {
        const result = await api.testServer(host, port);
        if (!result.reachable && !window.confirm(t("serverUnreachable"))) { setSaving(false); return; }
      } catch {
        if (!window.confirm(t("serverTestFailed"))) { setSaving(false); return; }
      }
    }

    setSaving(true);
    try {
      await api.setClient({ ...config, server_url: url });
    } catch (e) {
      console.error(e);
    } finally {
      setSaving(false);
    }
  }, [config, connectionMode, manualHost, manualPort, t]);

  return (
    <Card title={t("title")} id={cardId}>
      <div className="space-y-4">
        {/* Enable/disable client */}
        <div className="flex items-center justify-between">
          <label htmlFor={enableId} className="text-sm font-medium">{t("enableClient")}</label>
          <Switch id={enableId} checked={clientEnabled} onCheckedChange={async (checked) => {
            setClientEnabled(checked);
            try {
              if (checked) {
                // Enabling: clear the disabled sentinel so we don't re-persist it.
                const next = { ...config, server_url: config.server_url === "__disabled__" ? "" : config.server_url };
                setConfig(next);
                await api.setClient(next);
              } else {
                await api.setClient({ ...config, server_url: "__disabled__" });
              }
            } catch { setClientEnabled(!checked); }
          }} />
        </div>

        {serverRunning && !config.server_url && clientEnabled && (
          <div className="rounded-lg bg-primary/10 p-3 text-xs text-muted-foreground">
            {t("localServerHint")}
          </div>
        )}

        {clientEnabled && (
          <>
            {/* Connection Mode Segmented Toggle */}
            <div className="mb-4">
              <label className="mb-2 block text-xs font-semibold uppercase tracking-wider text-muted-foreground">{t("connectionMode")}</label>
              <div className="relative flex rounded-xl bg-muted p-1">
                {/* Active Highlight Slider */}
                <div
                  className="absolute top-1 bottom-1 rounded-lg bg-card shadow-xs transition-all duration-300 ease-out"
                  style={{
                    left: connectionMode === "auto" ? "4px" : "50%",
                    width: "calc(50% - 6px)",
                  }}
                />
                <button
                  type="button"
                  onClick={() => setConnectionMode("auto")}
                  className={`relative z-10 flex flex-1 items-center justify-center gap-2 py-2 text-sm font-semibold transition-colors ${
                    connectionMode === "auto" ? "text-foreground" : "text-muted-foreground hover:text-foreground"
                  }`}
                >
                  <svg className="size-4" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                    <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2.5} d="M13 10V3L4 14h7v7l9-11h-7z" />
                  </svg>
                  <span>{t("connectionModeAuto")}</span>
                </button>
                <button
                  type="button"
                  onClick={() => setConnectionMode("manual")}
                  className={`relative z-10 flex flex-1 items-center justify-center gap-2 py-2 text-sm font-semibold transition-colors ${
                    connectionMode === "manual" ? "text-foreground" : "text-muted-foreground hover:text-foreground"
                  }`}
                >
                  <svg className="size-4" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                    <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2.5} d="M10.325 4.317c.426-1.756 2.924-1.756 3.35 0a1.724 1.724 0 002.573 1.066c1.543-.94 3.31.826 2.37 2.37a1.724 1.724 0 001.065 2.572c1.756.426 1.756 2.924 0 3.35a1.724 1.724 0 00-1.066 2.573c.94 1.543-.826 3.31-2.37 2.37a1.724 1.724 0 00-2.572 1.065c-.426 1.756-2.924 1.756-3.35 0a1.724 1.724 0 00-2.573-1.066-1.543.94-3.31-.826-2.37-2.37 1.724 1.724 0 00-1.065-2.572c-1.756-.426-1.756-2.924 0-3.35a1.724 1.724 0 001.066-2.573c-.94-1.543.826-3.31 2.37-2.37.996.608 2.296.07 2.572-1.065z" />
                    <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2.5} d="M15 12a3 3 0 11-6 0 3 3 0 016 0z" />
                  </svg>
                  <span>{t("connectionModeManual")}</span>
                </button>
              </div>
            </div>

            {/* Panel 1: Automatic Discovery */}
            {connectionMode === "auto" && (
              <div className="space-y-3 animate-in fade-in duration-200">
                <div className="flex items-center justify-between">
                  <span className="text-xs font-semibold uppercase tracking-wider text-muted-foreground">{t("server")}</span>
                  <button
                    type="button"
                    onClick={scanForServers}
                    disabled={scanning}
                    className="inline-flex items-center gap-1.5 text-xs font-medium text-primary hover:text-primary/80 disabled:opacity-50"
                  >
                    {scanning ? (
                      <>
                        <svg className="size-3.5 animate-spin text-primary" fill="none" viewBox="0 0 24 24">
                          <circle className="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" strokeWidth="4" />
                          <path className="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z" />
                        </svg>
                        <span>{t("scanning")}</span>
                      </>
                    ) : (
                      <>
                        <svg className="size-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                          <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M4 4v5h.582m15.356 2A8.001 8.001 0 1121.21 8H17" />
                        </svg>
                        <span>{t("scanServers")}</span>
                      </>
                    )}
                  </button>
                </div>

                <div className="overflow-hidden rounded-xl border border-border bg-muted/10 shadow-xs">
                  {/* Zero-Config Auto Option */}
                  <button
                    type="button"
                    className={`relative flex w-full flex-col border-b border-border p-3.5 text-left transition-all ${
                      !config.server_url
                        ? "bg-primary/5 shadow-inner"
                        : "hover:bg-muted/40"
                    }`}
                    onClick={() => selectServer("")}
                  >
                    <div className="flex w-full items-center justify-between">
                      <div className="flex items-center gap-2.5">
                        {/* Animated Beacon/Radar Pulse Icon */}
                        <div className="relative flex size-3">
                          <span className={`absolute inline-flex h-full w-full animate-ping rounded-full opacity-75 ${!config.server_url ? "bg-primary" : "bg-muted-foreground/40"}`} />
                          <span className={`relative inline-flex size-3 rounded-full ${!config.server_url ? "bg-primary" : "bg-muted-foreground/60"}`} />
                        </div>
                        <span className={`text-sm font-semibold transition-colors ${!config.server_url ? "text-primary" : "text-foreground"}`}>
                          {t("autoConnectOption")}
                        </span>
                      </div>
                      {!config.server_url && (
                        <span className="flex size-5 items-center justify-center rounded-full bg-primary text-primary-foreground text-xs font-bold animate-in zoom-in duration-200">✓</span>
                      )}
                    </div>
                    <p className="mt-1 pl-5.5 text-xs text-muted-foreground">
                      {t("autoConnectDesc")}
                    </p>
                  </button>

                  {/* Discovered Servers List */}
                  {servers.length > 0 ? (
                    servers.map((s, idx) => {
                      const url = `tcp://${s.host}:${s.port}`;
                      const isSelected = config.server_url === url;
                      return (
                        <button
                          key={s.host}
                          type="button"
                          className={`flex w-full items-center justify-between p-3.5 text-left transition-all ${
                            idx !== servers.length - 1 ? "border-b border-border" : ""
                          } ${isSelected ? "bg-primary/5 font-semibold shadow-inner" : "hover:bg-muted/40"}`}
                          onClick={() => selectServer(url)}
                        >
                          <div className="flex items-center gap-3">
                            <div className={`flex size-8 items-center justify-center rounded-lg ${isSelected ? "bg-primary/10 text-primary" : "bg-muted text-muted-foreground"}`}>
                              <svg className="size-4" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                                <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M5 12h14M5 12a2 2 0 01-2-2V6a2 2 0 012-2h14a2 2 0 012 2v4a2 2 0 01-2 2M5 12a2 2 0 00-2 2v4a2 2 0 002 2h14a2 2 0 002-2v-4a2 2 0 00-2-2m-2-4h.01M17 16h.01" />
                              </svg>
                            </div>
                            <div>
                              <span className={`text-sm font-semibold transition-colors ${isSelected ? "text-primary" : "text-foreground"}`}>
                                {s.name}
                              </span>
                              <span className="ml-2 font-mono text-xs text-muted-foreground">{s.host}:{s.port}</span>
                            </div>
                          </div>
                          {isSelected && (
                            <span className="flex size-5 items-center justify-center rounded-full bg-primary text-primary-foreground text-xs font-bold animate-in zoom-in duration-200">✓</span>
                          )}
                        </button>
                      );
                    })
                  ) : (
                    !scanning && (
                      <div className="flex flex-col items-center justify-center p-6 text-center text-muted-foreground animate-in fade-in duration-200">
                        <svg className="mb-2 size-8 text-muted-foreground/40" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                          <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={1.5} d="M21 21l-6-6m2-5a7 7 0 11-14 0 7 7 0 0114 0z" />
                        </svg>
                        <p className="text-xs">{t("noServersFound")}</p>
                      </div>
                    )
                  )}
                </div>
              </div>
            )}

            {/* Panel 2: Manual Configuration */}
            {connectionMode === "manual" && (
              <div className="space-y-3.5 animate-in fade-in duration-200">
                <span className="text-xs font-semibold uppercase tracking-wider text-muted-foreground">{t("manualSettings")}</span>
                
                <div className="rounded-xl border border-border bg-muted/5 p-4 shadow-xs space-y-4">
                  <div className="flex gap-3">
                    <div className="flex-1 space-y-1.5">
                      <label className="text-xs font-medium text-muted-foreground" htmlFor="manual-host">{t("serverAddress")}</label>
                      <Input
                        id="manual-host"
                        value={manualHost}
                        onChange={(e) => handleManualHostChange(e.target.value)}
                        placeholder={t("manualPlaceholder")}
                        className="h-10 text-sm"
                        aria-label={t("serverAddress")}
                      />
                    </div>
                    
                    <div className="w-24 space-y-1.5">
                      <label className="text-xs font-medium text-muted-foreground" htmlFor="manual-port">{t("port")}</label>
                      <Input
                        id="manual-port"
                        value={manualPort}
                        onChange={(e) => handleManualPortChange(e.target.value)}
                        className="h-10 text-sm font-mono text-center"
                        aria-label={t("port")}
                      />
                    </div>
                  </div>

                  {/* Computed Connection String and Test Button */}
                  {manualHost && (
                    <div className="flex flex-col sm:flex-row gap-3 sm:items-center sm:justify-between rounded-lg bg-muted/30 p-3 border border-border/50 animate-in slide-in-from-top-2 duration-200">
                      <div className="space-y-0.5">
                        <span className="text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">Target URL</span>
                        <div className="font-mono text-xs text-foreground bg-background px-2.5 py-1.5 rounded border border-border/70 shadow-xs">
                          tcp://{manualHost}:{manualPort || "1704"}
                        </div>
                      </div>

                      <div className="flex items-center gap-2 self-end sm:self-auto">
                        {testStatus === "success" && (
                          <span className="inline-flex items-center gap-1.5 text-xs font-semibold text-emerald-500 bg-emerald-500/10 px-2.5 py-1 rounded-full border border-emerald-500/20 animate-in zoom-in duration-200">
                            <svg className="size-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                              <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2.5} d="M5 13l4 4L19 7" />
                            </svg>
                            <span>{t("connectionSuccess")}</span>
                          </span>
                        )}
                        {testStatus === "failed" && (
                          <span className="inline-flex items-center gap-1.5 text-xs font-semibold text-rose-500 bg-rose-500/10 px-2.5 py-1 rounded-full border border-rose-500/20 animate-in zoom-in duration-200">
                            <svg className="size-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                              <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2.5} d="M6 18L18 6M6 6l12 12" />
                            </svg>
                            <span>{t("connectionFailed")}</span>
                          </span>
                        )}
                        
                        <Button
                          size="sm"
                          type="button"
                          variant="outline"
                          onClick={testManualConnection}
                          disabled={testStatus === "testing"}
                          className="h-8 text-xs px-3 shadow-xs"
                        >
                          {testStatus === "testing" ? (
                            <>
                              <svg className="size-3 animate-spin mr-1.5 text-foreground" fill="none" viewBox="0 0 24 24">
                                <circle className="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" strokeWidth="4" />
                                <path className="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z" />
                              </svg>
                              <span>{t("testingConnection")}</span>
                            </>
                          ) : (
                            t("testConnection")
                          )}
                        </Button>
                      </div>
                    </div>
                  )}
                </div>
              </div>
            )}

            <Field label={t("hostId")} htmlFor={hostIdFieldId}>
              <Input id={hostIdFieldId} value={config.host_id} onChange={(e) => setConfig({ ...config, host_id: e.target.value })} placeholder="kitchen" />
            </Field>
            
            <Field label={t("soundcard")} htmlFor={soundcardId}>
              <Select
                id={soundcardId}
                value={soundcardCustom ? "__custom__" : config.soundcard}
                onChange={(e) => {
                  const v = e.target.value;
                  if (v === "__custom__") {
                    setManualCustomSoundcard(true);
                  } else {
                    setManualCustomSoundcard(false);
                    setConfig({ ...config, soundcard: v });
                  }
                }}
              >
                <option value="default">{t("defaultSoundcard")}</option>
                {soundcards.map((sc) => (
                  <option key={sc.device} value={sc.device}>{sc.name} ({sc.device})</option>
                ))}
                <option value="__custom__">{t("customSoundcard")}</option>
              </Select>
              {soundcardCustom && (
                <Input
                  className="mt-2"
                  value={config.soundcard === "default" ? "" : config.soundcard}
                  onChange={(e) => setConfig({ ...config, soundcard: e.target.value })}
                  placeholder="hw:0"
                />
              )}
            </Field>
            
            <Field label={t("mixer")} htmlFor={mixerId}>
              <Select id={mixerId} value={config.mixer} onChange={(e) => setConfig({ ...config, mixer: e.target.value })}>
                <option value="software">{t("mixerSoftware")}</option>
                <option value="hardware">{t("mixerHardware")}</option>
                <option value="midi">{t("mixerMidi")}</option>
                <option value="none">{t("mixerNone")}</option>
              </Select>
            </Field>
            
            <Field label={t("latency")} htmlFor={latencyId}>
              <Input id={latencyId} type="number" inputMode="numeric" min={0} value={config.latency} onChange={(e) => setConfig({ ...config, latency: Number(e.target.value) })} />
              <p className="text-xs text-muted-foreground">{t("latencyHint")}</p>
            </Field>
            
            <Button size="sm" onClick={saveConfig} disabled={saving}>
              {saving ? t("testing") : t("save")}
            </Button>
          </>
        )}
      </div>
    </Card>
  );
}

function SshTab() {
  const t = useTranslations("ssh");
  const [config, setConfig] = useState<SshConfig>({ enabled: false, pubkeys: [] });
  const switchId = useId();
  const keysId = useId();
  const cardId = useId();

  useEffect(() => {
    api.getSsh().then(setConfig).catch(() => {});
  }, []);

  return (
    <Card title={t("title")} id={cardId}>
      <div className="space-y-4">
        <div className="flex items-center justify-between">
          <label htmlFor={switchId} className="text-sm">
            {t("enable")}
          </label>
          <Switch
            id={switchId}
            checked={config.enabled}
            onCheckedChange={(checked) => setConfig({ ...config, enabled: checked })}
            aria-describedby={`${switchId}-desc`}
          />
          <span id={`${switchId}-desc`} className="sr-only">
            {t("enableDescription")}
          </span>
        </div>
        <div className="flex flex-col gap-1.5">
          <label htmlFor={keysId} className="text-sm text-muted-foreground">
            {t("authorizedKeys")}
          </label>
          <textarea
            id={keysId}
            className="h-32 w-full resize-none rounded-xl border border-input bg-input/30 px-3 py-2 font-mono text-xs outline-none focus-visible:border-ring focus-visible:ring-[3px] focus-visible:ring-ring/50"
            value={config.pubkeys.join("\n")}
            onChange={(e) => setConfig({ ...config, pubkeys: e.target.value.split("\n").filter(Boolean) })}
            aria-label={t("authorizedKeys")}
            spellCheck={false}
            autoComplete="off"
          />
        </div>
        <Button size="sm" onClick={() => { if (config.enabled && config.pubkeys.length === 0) { alert(t("pubkeyRequired")); return; } api.setSsh(config); }}>
          {t("save")}
        </Button>
      </div>
    </Card>
  );
}

// ── System Tab ────────────────────────────────────────────────

function TimezoneCard() {
  const t = useTranslations("system");
  const [timezone, setTimezone] = useState("");
  const [available, setAvailable] = useState<string[]>([]);
  const [status, setStatus] = useState<"idle" | "saving" | "saved">("idle");
  const [error, setError] = useState("");
  const tzId = useId();
  const cardId = useId();

  useEffect(() => {
    api.getTimezone().then((data) => {
      setTimezone(data.timezone);
      setAvailable(data.available);
    }).catch(() => {});
  }, []);

  // Timezone is saved on selection (no separate Save button); surface the result
  // so the change is visibly confirmed and failures don't pass silently.
  const save = async (tz: string) => {
    const previous = timezone;
    setTimezone(tz);
    setError("");
    setStatus("saving");
    try {
      await api.setTimezone(tz);
      setStatus("saved");
      setTimeout(() => setStatus("idle"), 2000);
    } catch (e) {
      setTimezone(previous); // revert to the last persisted value
      setStatus("idle");
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  if (!available.length) return null;

  return (
    <Card title={t("timezone")} id={cardId}>
      <Field label={t("timezoneSelect")} htmlFor={tzId}>
        <Select id={tzId} value={timezone} disabled={status === "saving"} onChange={(e) => void save(e.target.value)}>
          {available.map((tz) => (
            <option key={tz} value={tz}>{tz}</option>
          ))}
        </Select>
      </Field>
      {error && (
        <div className="mt-2 rounded-lg bg-destructive/10 px-3 py-2 text-xs text-destructive" role="status">
          <p className="font-medium">{t("timezoneSaveError")}</p>
          <p className="mt-0.5 opacity-90">{error}</p>
        </div>
      )}
      {status === "saved" && !error && <p className="mt-2 text-xs text-green-600">{t("saved")}</p>}
    </Card>
  );
}

function LogsCard() {
  const t = useTranslations("system");
  const [logs, setLogs] = useState<string[]>([]);
  const [expanded, setExpanded] = useState(false);
  const [filter, setFilter] = useState("all");
  const [copied, setCopied] = useState(false);
  const cardId = useId();
  const filterId = useId();

  const fetchLogs = useCallback(() => {
    const url = filter === "all" ? "/api/system/logs" : `/api/system/logs?service=${filter}`;
    fetch(url).then(r => r.json()).then((data) => {
      setLogs(data.lines || []);
    }).catch(() => {});
  }, [filter]);

  useEffect(() => { fetchLogs(); }, [fetchLogs]);

  const copyLogs = async () => {
    if (!logs.length) return;
    const text = logs.join("\n");
    try {
      if (navigator.clipboard && window.isSecureContext) {
        await navigator.clipboard.writeText(text);
      } else {
        // The device is reached over plain HTTP on the LAN, which is not a secure
        // context — navigator.clipboard is unavailable there, so fall back to a
        // hidden textarea + execCommand("copy").
        const ta = document.createElement("textarea");
        ta.value = text;
        ta.style.position = "fixed";
        ta.style.opacity = "0";
        document.body.appendChild(ta);
        ta.focus();
        ta.select();
        document.execCommand("copy");
        document.body.removeChild(ta);
      }
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    } catch {
      /* clipboard unsupported — leave the label unchanged */
    }
  };

  return (
    <Card title={t("logs")} id={cardId}>
      <div className="space-y-2">
        <div className="flex flex-wrap items-end gap-2">
          <div className="flex gap-2">
            <Button variant="outline" size="sm" onClick={fetchLogs}>{t("refreshLogs")}</Button>
            <Button variant="outline" size="sm" onClick={() => setExpanded(!expanded)}>
              {expanded ? t("collapseLogs") : t("expandLogs")}
            </Button>
            <Button variant="outline" size="sm" onClick={copyLogs} disabled={!logs.length}>
              {copied ? t("copied") : t("copyLogs")}
            </Button>
          </div>
          <div className="flex-1 min-w-[200px]">
            <Select id={filterId} value={filter} onChange={(e) => setFilter(e.target.value)}>
              <option value="all">{t("logAll")}</option>
              <option value="server">{t("logServer")}</option>
              <option value="client">{t("logClient")}</option>
              <option value="controller">{t("logController")}</option>
            </Select>
          </div>
        </div>
        <pre
          className={`overflow-x-auto rounded-lg bg-muted p-3 font-mono text-[10px] leading-tight text-muted-foreground ${expanded ? "max-h-96" : "max-h-32"} overflow-y-auto`}
          aria-label={t("logs")}
        >
          {logs.length ? logs.join("\n") : t("noLogs")}
        </pre>
      </div>
    </Card>
  );
}

function UpdateTab() {
  const t = useTranslations("update");
  const [update, setUpdate] = useState<import("@/lib/api").UpdateCheck | null>(null);
  const [checking, setChecking] = useState(false);
  const [phase, setPhase] = useState<"idle" | "uploading" | "downloading" | "verifying" | "installing" | "rebooting" | "reconnecting" | "done" | "failed">("idle");
  const [rolledBack, setRolledBack] = useState(false);
  const [confirming, setConfirming] = useState(false);
  const [progress, setProgress] = useState<number | null>(null);
  const [errorMsg, setErrorMsg] = useState<string | null>(null);
  const cardId = useId();

  const fileInputRef = useRef<HTMLInputElement>(null);
  const [selectedFile, setSelectedFile] = useState<File | null>(null);
  const [showWarningGate, setShowWarningGate] = useState(false);
  const [acceptedRisks, setAcceptedRisks] = useState(false);
  // null = not uploading; 0..1 = fraction sent (or -1 when total is unknown).
  const [uploadFraction, setUploadFraction] = useState<number | null>(null);
  const [uploadError, setUploadError] = useState<string | null>(null);

  useEffect(() => {
    api.checkUpdate().then(setUpdate).catch(() => {});
    api.getUpdateStatus().then((s) => { if (s.rolled_back) setRolledBack(true); }).catch(() => {});
  }, []);

  const checkForUpdate = useCallback(() => {
    setChecking(true);
    api.checkUpdate().then(setUpdate).catch(() => {}).finally(() => setChecking(false));
  }, []);

  // Poll GET /update/status until the async RAUC install truly completes. RAUC's
  // InstallBundle returns on TRIGGER, not completion, so both install paths must
  // watch the operation transition installing→idle (and surface last_error) rather
  // than treating the 202 as "done". Resolves on success, rejects with the reason.
  const pollInstallToCompletion = useCallback(
    () =>
      new Promise<void>((resolve, reject) => {
        const started = Date.now();
        let sawInstalling = false;
        const poll = setInterval(async () => {
          if (Date.now() - started > 20 * 60 * 1000) {
            clearInterval(poll);
            reject(new Error(t("updateTimeout")));
            return;
          }
          try {
            const s = await api.getUpdateStatus();
            if (s.last_error) {
              clearInterval(poll);
              reject(new Error(s.last_error));
            } else if (s.operation === "installing") {
              sawInstalling = true;
              setProgress(s.progress?.percentage ?? null);
              // Reflect RAUC's sub-step so the indicator isn't stuck on "installing".
              const m = (s.progress?.message ?? "").toLowerCase();
              if (m.includes("download")) setPhase("downloading");
              else if (m.includes("verif") || m.includes("check") || m.includes("signature")) setPhase("verifying");
              else setPhase("installing");
            } else if (s.operation === "idle" && sawInstalling) {
              // Written + verified to the inactive slot; activated via "Reboot now".
              clearInterval(poll);
              resolve();
            } else if (s.operation === "idle" && !sawInstalling && Date.now() - started > 20000) {
              // 20s after trigger and RAUC never entered "installing" → it never
              // started (unreachable bundle / refused); fail instead of hang or lie.
              clearInterval(poll);
              reject(new Error(t("updateFailed")));
            }
          } catch {
            /* device busy mid-install — keep polling */
          }
        }, 1500);
      }),
    [t],
  );

  const performUpdate = useCallback(() => {
    setPhase("installing");
    setProgress(null);
    setErrorMsg(null);
    // triggerUpdate returns once the install is TRIGGERED; poll for real completion.
    api.triggerUpdate()
      .then(() => pollInstallToCompletion())
      .then(() => setPhase("done"))
      .catch((e: unknown) => {
        setErrorMsg(e instanceof Error ? e.message : String(e));
        setPhase("failed");
      });
  }, [pollInstallToCompletion]);

  // Activate the freshly-installed slot and confirm the device returns. Drives
  // rebooting → reconnecting, waits for the device to actually drop then come back,
  // and reports a rollback (bootloader reverted) instead of silently going idle.
  const rebootAndVerify = useCallback(() => {
    setPhase("rebooting");
    setErrorMsg(null);
    api.reboot().catch(() => {}); // the reboot kills the connection — expected.
    setPhase("reconnecting");
    const started = Date.now();
    let sawOffline = false;
    const poll = setInterval(async () => {
      if (Date.now() - started > 3 * 60 * 1000) {
        clearInterval(poll);
        setErrorMsg(t("reconnectTimeout"));
        setPhase("failed");
        return;
      }
      try {
        const s = await api.getUpdateStatus();
        // Only trust an "online" reading after we've seen it go offline, else we'd
        // match the still-running pre-reboot instance and finish too early.
        if (!sawOffline) return;
        clearInterval(poll);
        if (s.rolled_back) {
          setRolledBack(true);
          setErrorMsg(t("rollbackDetail"));
          setPhase("failed");
        } else {
          api.checkUpdate().then(setUpdate).catch(() => {});
          setPhase("idle");
        }
      } catch {
        sawOffline = true; // device went down — reboot in progress
      }
    }, 2000);
  }, [t]);

  const triggerFileSelect = useCallback(() => {
    fileInputRef.current?.click();
  }, []);

  const handleFileSelected = useCallback((e: React.ChangeEvent<HTMLInputElement>) => {
    const file = e.target.files?.[0];
    if (file) {
      setSelectedFile(file);
      setShowWarningGate(true);
      setAcceptedRisks(false);
      setUploadError(null);
    }
  }, []);

  const handleFileDrop = useCallback((e: React.DragEvent<HTMLDivElement>) => {
    e.preventDefault();
    const file = e.dataTransfer.files?.[0];
    if (file) {
      setSelectedFile(file);
      setShowWarningGate(true);
      setAcceptedRisks(false);
      setUploadError(null);
    }
  }, []);

  const startManualFlash = useCallback(() => {
    if (!selectedFile) return;
    let stage: "upload" | "install" = "upload";
    setUploadFraction(-1);
    setUploadError(null);
    setPhase("uploading");
    // Stage 1: upload (with progress). A failure here keeps the modal open + inline.
    api.uploadUpdate(selectedFile, (f) => setUploadFraction(f ?? -1))
      .then(() => {
        // Upload done — close the modal, move into the install lifecycle.
        setShowWarningGate(false);
        setSelectedFile(null);
        setAcceptedRisks(false);
        setUploadFraction(null);
        setPhase("installing");
        setProgress(null);
        setErrorMsg(null);
        stage = "install";
        // Stage 2: trigger the async install, then poll to REAL completion instead
        // of declaring "done" on the 202 while rauc is still writing the slot.
        return api.installUpdate().then(() => pollInstallToCompletion());
      })
      .then(() => setPhase("done"))
      .catch((err: unknown) => {
        const msg = err instanceof Error ? err.message : String(err);
        if (stage === "upload") {
          setUploadError(t("uploadError"));
          setUploadFraction(null);
        } else {
          // Install-stage failure — modal already closed, surface in the panel.
          setErrorMsg(msg);
          setPhase("failed");
        }
      });
  }, [selectedFile, pollInstallToCompletion, t]);

  return (
    <Card title={t("title")} id={cardId}>
      <div className="space-y-4">
        {rolledBack && phase !== "done" && (
          <div className="rounded-lg bg-destructive/10 p-4 text-sm" role="alert">
            <p className="font-medium text-destructive">{t("rollbackWarning")}</p>
            <p className="mt-1 text-xs text-muted-foreground">{t("rollbackDetail")}</p>
          </div>
        )}

        {phase !== "idle" && phase !== "done" && phase !== "failed" && (
          <UpdatePhaseIndicator label={`${t(`phase_${phase}`)}${
            phase === "uploading" && uploadFraction != null && uploadFraction >= 0
              ? ` — ${Math.round(uploadFraction * 100)}%`
              : progress != null ? ` — ${progress}%` : ""
          }`} />
        )}
        {phase === "done" && (
          <div className="rounded-lg bg-green-500/10 p-4 text-sm space-y-3" role="status">
            <p className="font-medium text-green-700 dark:text-green-400">{t("updateSuccess")}</p>
            <p className="text-xs text-muted-foreground">{t("rebootToActivate")}</p>
            <div className="flex gap-2">
              <Button size="sm" onClick={rebootAndVerify}>{t("rebootNow")}</Button>
              <Button variant="outline" size="sm" onClick={() => setPhase("idle")}>{t("later")}</Button>
            </div>
          </div>
        )}
        {phase === "failed" && !rolledBack && (
          <div className="rounded-lg bg-destructive/10 p-4 text-sm space-y-1" role="alert">
            <p className="font-medium text-destructive">{t("updateFailed")}</p>
            {errorMsg && <p className="text-xs text-muted-foreground break-words">{errorMsg}</p>}
          </div>
        )}

        {phase === "idle" && (
          <>
            {confirming ? (
              <div className="flex flex-col gap-3 rounded-lg bg-primary/10 p-4" role="alertdialog" aria-label={t("updateConfirm")}>
                <p className="text-sm font-medium">{t("updateConfirm")}</p>
                {update?.latest_version && (
                  <p className="text-xs font-mono text-muted-foreground">{update.current_version} → {update.latest_version}</p>
                )}
                {update && !update.signature_verified && (
                  <p className="text-xs font-medium text-destructive">{t("signatureUnverified")}</p>
                )}
                <div className="flex gap-2">
                  <Button size="sm" onClick={() => { setConfirming(false); performUpdate(); }}>{t("confirmInstall")}</Button>
                  <Button variant="outline" size="sm" onClick={() => setConfirming(false)}>{t("cancel")}</Button>
                </div>
              </div>
            ) : update?.available ? (
              <div className="flex flex-col gap-3 rounded-lg bg-primary/10 p-4">
                <div className="flex items-center justify-between">
                  <div>
                    <p className="text-sm font-medium">{t("updateAvailable")}</p>
                    <p className="text-xs text-muted-foreground">{update.current_version}{update.latest_version ? ` → ${update.latest_version}` : ""}</p>
                  </div>
                  <Button size="sm" disabled={!update.installable} onClick={() => setConfirming(true)}>{t("installUpdate")}</Button>
                </div>
                <div className={`text-xs font-semibold flex items-center gap-1 border-t pt-2 ${update.signature_verified ? "border-primary/20 text-green-600 dark:text-green-400" : "border-destructive/30 text-destructive"}`}>
                  {update.signature_verified ? t("signatureVerified") : t("signatureUnverified")}
                </div>
              </div>
            ) : update?.is_downgrade ? (
              <div className="flex flex-col gap-3 rounded-lg bg-muted p-4">
                <div className="flex items-center justify-between">
                  <div>
                    <p className="text-sm font-medium">{t("downgradeAvailable")}</p>
                    <p className="text-xs text-muted-foreground">{update.current_version}{update.latest_version ? ` → ${update.latest_version}` : ""}</p>
                  </div>
                  <Button variant="outline" size="sm" disabled={!update.installable} onClick={() => setConfirming(true)}>{t("installVersion")}</Button>
                </div>
                <div className={`text-xs font-semibold flex items-center gap-1 border-t pt-2 ${update.signature_verified ? "border-border text-green-600 dark:text-green-400" : "border-destructive/30 text-destructive"}`}>
                  {update.signature_verified ? t("signatureVerified") : t("signatureUnverified")}
                </div>
              </div>
            ) : update && update.latest_version ? (
              <div className="flex flex-col gap-2 rounded-lg bg-muted/20 p-4">
                <div className="flex items-center gap-2 text-sm text-muted-foreground">
                  <StatusDot connected label={t("upToDate")} />
                  <span>{t("upToDate")}</span>
                </div>
                <div className="text-xs font-semibold flex items-center gap-1 text-green-600 dark:text-green-400 border-t border-border/50 pt-2">
                  {t("signatureVerified")}
                </div>
              </div>
            ) : update ? (
              <div className="rounded-lg bg-muted/20 p-4 text-sm text-muted-foreground" role="status">
                {t("serverUnreachable")}
              </div>
            ) : null}
            <Button variant="outline" size="sm" onClick={checkForUpdate} disabled={checking} aria-busy={checking}>
              {checking ? t("checking") : t("checkNow")}
            </Button>
          </>
        )}

        {/* Channel lives in AutoUpdateSettings (always visible) — it is the single
            source of truth the backend uses for both manual check/install and
            auto-update, so there is no separate decoupled selector here. */}
        <AutoUpdateSettings />

        {phase === "idle" && (
          <>
            <hr className="border-border/50" />
            <div className="space-y-3">
              <div>
                <h3 className="text-sm font-semibold">{t("manualTitle")}</h3>
                <p className="text-xs text-muted-foreground mt-0.5">{t("manualDesc")}</p>
              </div>
              <div
                className="border-2 border-dashed border-border/80 hover:border-primary/50 transition rounded-lg p-6 flex flex-col items-center justify-center cursor-pointer space-y-2 bg-muted/20"
                onClick={triggerFileSelect}
                onDragOver={(e) => e.preventDefault()}
                onDrop={handleFileDrop}
              >
                <svg className="size-8 text-muted-foreground" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                  <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={1.5} d="M7 16a4 4 0 01-.88-7.903A5 5 0 1115.9 6L16 6a5 5 0 011 9.9M15 13l-3-3m0 0l-3 3m3-3v12" />
                </svg>
                <span className="text-xs font-semibold text-muted-foreground">{t("manualUploadButton")}</span>
                <input
                  type="file"
                  ref={fileInputRef}
                  className="hidden"
                  accept=".raucb"
                  onChange={handleFileSelected}
                />
              </div>
            </div>
          </>
        )}
      </div>

      {showWarningGate && selectedFile && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 backdrop-blur-md p-4 animate-in fade-in duration-200">
          <div className="w-full max-w-lg rounded-xl border border-destructive/30 bg-background/95 shadow-2xl p-6 space-y-6 max-h-[90vh] overflow-y-auto transform scale-100 transition duration-200">
            <div className="space-y-2">
              <h2 className="text-base font-bold text-destructive flex items-center gap-2">
                <span>{t("manualWarningTitle")}</span>
              </h2>
              <p className="text-xs text-foreground/90 leading-relaxed font-semibold">
                {t("manualWarningDesc1")}
              </p>
              <p className="text-xs text-foreground/90 leading-relaxed font-semibold">
                {t("manualWarningDesc2")}
              </p>
              <p className="text-xs text-muted-foreground font-mono bg-muted/60 p-2 rounded border border-border/50">
                File: {selectedFile.name} ({(selectedFile.size / 1024 / 1024).toFixed(2)} MB)
              </p>
            </div>

            <div className="flex items-start gap-3 rounded-lg border border-border/85 bg-muted/40 p-4">
              <input
                id="accept-risks-checkbox"
                type="checkbox"
                checked={acceptedRisks}
                onChange={(e) => setAcceptedRisks(e.target.checked)}
                className="mt-1 size-4 rounded border-border text-primary focus:ring-primary cursor-pointer"
              />
              <label htmlFor="accept-risks-checkbox" className="text-xs font-bold text-foreground/80 cursor-pointer select-none leading-relaxed">
                {t("manualConfirmCheckbox")}
              </label>
            </div>

            {uploadError && (
              <div className="rounded-lg bg-destructive/10 p-3 text-xs text-destructive font-semibold">
                {uploadError}
              </div>
            )}

            <div className="flex justify-end gap-3 pt-2">
              <Button
                variant="outline"
                onClick={() => {
                  setShowWarningGate(false);
                  setSelectedFile(null);
                  setAcceptedRisks(false);
                  setUploadError(null);
                  if (fileInputRef.current) fileInputRef.current.value = "";
                }}
                disabled={uploadFraction !== null}
              >
                {t("manualCancel")}
              </Button>
              <Button
                variant="destructive"
                onClick={startManualFlash}
                disabled={!acceptedRisks || uploadFraction !== null}
                className="font-bold"
              >
                {uploadFraction !== null
                  ? uploadFraction >= 0
                    ? `${t("manualUploading")} ${Math.round(uploadFraction * 100)}%`
                    : t("manualUploading")
                  : t("manualProceed")}
              </Button>
            </div>
          </div>
        </div>
      )}
      <RawFlashSection />
    </Card>
  );
}

function RawFlashSection() {
  const [expanded, setExpanded] = useState(false);
  const [file, setFile] = useState<File | null>(null);
  const [challenge, setChallenge] = useState<string | null>(null);
  const [input, setInput] = useState("");
  const [status, setStatus] = useState<"idle" | "uploading" | "confirming" | "flashing" | "done" | "error">("idle");
  const id = useId();

  const handleUpload = async () => {
    if (!file) return;
    setStatus("uploading");
    try {
      const res = await api.flashRawUpload(file);
      setChallenge(res.challenge);
      setStatus("confirming");
    } catch { setStatus("error"); }
  };

  const handleConfirm = async () => {
    if (!challenge || input.toUpperCase() !== challenge) return;
    setStatus("flashing");
    try {
      await api.flashRawConfirm(challenge);
      setStatus("done");
    } catch { setStatus("error"); }
  };

  return (
    <div className="border-t border-border pt-3 mt-3">
      <button type="button" onClick={() => setExpanded(!expanded)} className="text-xs text-muted-foreground hover:text-foreground">
        ▸ Advanced: Raw Flash (escape hatch)
      </button>
      {expanded && (
        <div className="mt-3 space-y-3 rounded-lg border border-destructive/30 bg-destructive/5 p-3">
          <p className="text-xs text-destructive font-medium">⚠️ Bypasses signature verification. Use only for recovery or development.</p>
          {status === "idle" && (
            <>
              <div>
                <label htmlFor={id} className="text-xs font-medium">Root filesystem image (.img or .img.gz)</label>
                <input id={id} type="file" accept=".img,.img.gz,.gz" onChange={(e) => setFile(e.target.files?.[0] ?? null)} className="mt-1 block w-full text-xs" />
              </div>
              <Button size="sm" variant="outline" disabled={!file} onClick={handleUpload}>Upload &amp; prepare</Button>
            </>
          )}
          {status === "uploading" && <p className="text-xs">Uploading...</p>}
          {status === "confirming" && challenge && (
            <div className="space-y-2">
              <p className="text-xs">Type <code className="font-bold text-destructive">{challenge}</code> to confirm flash:</p>
              <Input value={input} onChange={(e) => setInput(e.target.value.toUpperCase())} placeholder={challenge} className="font-mono text-center" />
              <Button size="sm" variant="outline" disabled={input !== challenge} onClick={handleConfirm}>Confirm flash</Button>
            </div>
          )}
          {status === "flashing" && <p className="text-xs">Flashing to inactive partition...</p>}
          {status === "done" && (
            <div className="space-y-2">
              <p className="text-xs text-green-600">Flash complete. Reboot to activate.</p>
              <Button size="sm" onClick={() => api.reboot()}>Reboot now</Button>
            </div>
          )}
          {status === "error" && <p className="text-xs text-destructive">Failed. Challenge expired or upload error.</p>}
        </div>
      )}
    </div>
  );
}

function AutoUpdateSettings() {
  const t = useTranslations("update");
  const [config, setConfig] = useState({ enabled: true, channel: "release", interval: "daily", time: "04:00" });
  const channelId = useId();
  const intervalId = useId();
  const timeId = useId();

  useEffect(() => {
    api.getAutoUpdate().then(setConfig).catch(() => {});
  }, []);

  const save = (updated: typeof config) => {
    setConfig(updated);
    api.setAutoUpdate(updated);
  };
  // The channel is the single source of truth the backend uses for BOTH manual
  // check/install and auto-update, so it is always visible (not gated on the
  // auto-update toggle). Also mirror it to the OS channel file so the settings
  // export / system view stay consistent.
  const saveChannel = (channel: string) => {
    save({ ...config, channel });
    api.setSystem({ channel }).catch(() => {});
  };

  return (
    <div className="space-y-3 border-t border-border pt-3">
      <Field label={t("channel")} htmlFor={channelId}>
        <Select id={channelId} value={config.channel} onChange={(e) => saveChannel(e.target.value)}>
          <option value="release">{t("stable")}</option>
          <option value="beta">{t("beta")}</option>
        </Select>
      </Field>
      <div className="flex items-center justify-between">
        <span className="text-sm text-muted-foreground">{t("autoUpdate")}</span>
        <Switch checked={config.enabled} onCheckedChange={(enabled) => save({ ...config, enabled })} />
      </div>
      {config.enabled && (
        <>
          <Field label={t("checkInterval")} htmlFor={intervalId}>
            <Select id={intervalId} value={config.interval} onChange={(e) => save({ ...config, interval: e.target.value })}>
              <option value="daily">{t("daily")}</option>
              <option value="weekly">{t("weekly")}</option>
              <option value="monthly">{t("monthly")}</option>
            </Select>
          </Field>
          <Field label={t("updateTime")} htmlFor={timeId}>
            <Input id={timeId} type="time" value={config.time} onChange={(e) => save({ ...config, time: e.target.value })} />
          </Field>
        </>
      )}
    </div>
  );
}

function UpdatePhaseIndicator({ label }: { label: string }) {
  return (
    <div className="flex items-center gap-3 rounded-lg bg-primary/10 p-4" role="status" aria-live="polite">
      <div className="size-4 animate-spin rounded-full border-2 border-primary border-t-transparent" />
      <span className="text-sm font-medium">{label}</span>
    </div>
  );
}

function DevicePasswordCard() {
  const [currentPw, setCurrentPw] = useState("");
  const [newPw, setNewPw] = useState("");
  const [confirmPw, setConfirmPw] = useState("");
  const [status, setStatus] = useState<"idle" | "saving" | "saved" | "error">("idle");
  const [authEnabled, setAuthEnabled] = useState(false);
  const id = useId();

  useEffect(() => {
    api.getAuthStatus().then((s) => setAuthEnabled(s.enabled)).catch(() => {});
  }, []);

  const handleSave = async () => {
    if (newPw !== confirmPw) { setStatus("error"); return; }
    setStatus("saving");
    try {
      await api.setPassword(authEnabled ? currentPw : null, newPw || null);
      setStatus("saved");
      setCurrentPw(""); setNewPw(""); setConfirmPw("");
      setAuthEnabled(!!newPw);
      setTimeout(() => setStatus("idle"), 2000);
    } catch { setStatus("error"); }
  };

  return (
    <Card title="Device Password" id={id}>
      <div className="space-y-3">
        <p className="text-sm text-muted-foreground">
          Protects the web UI and console login. Leave empty to disable.
        </p>
        {authEnabled && (
          <div>
            <label htmlFor={`${id}-current`} className="text-sm font-medium">Current password</label>
            <Input id={`${id}-current`} type="password" value={currentPw} onChange={(e) => setCurrentPw(e.target.value)} />
          </div>
        )}
        <div>
          <label htmlFor={`${id}-new`} className="text-sm font-medium">New password</label>
          <Input id={`${id}-new`} type="password" value={newPw} onChange={(e) => setNewPw(e.target.value)} placeholder="Leave empty to disable" />
        </div>
        <div>
          <label htmlFor={`${id}-confirm`} className="text-sm font-medium">Confirm password</label>
          <Input id={`${id}-confirm`} type="password" value={confirmPw} onChange={(e) => setConfirmPw(e.target.value)} />
        </div>
        {status === "error" && <p className="text-sm text-destructive">Passwords don&apos;t match or current password is wrong.</p>}
        {status === "saved" && <p className="text-sm text-green-600">Password updated.</p>}
        <Button onClick={handleSave} disabled={status === "saving" || (newPw !== confirmPw)}>
          {status === "saving" ? "Saving..." : "Save"}
        </Button>
      </div>
    </Card>
  );
}

function SettingsCard() {
  const t = useTranslations("system");
  const cardId = useId();
  const fileInputRef = useRef<HTMLInputElement>(null);
  const [preview, setPreview] = useState<import("@/lib/api").SettingsPreview | null>(null);
  const [selectedFile, setSelectedFile] = useState<File | null>(null);
  const [importing, setImporting] = useState(false);

  const handleExport = async () => {
    const blob = await api.exportSettings();
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = "snapdog-settings.tar.gz";
    a.click();
    URL.revokeObjectURL(url);
  };

  const handleFileSelect = async (e: React.ChangeEvent<HTMLInputElement>) => {
    const file = e.target.files?.[0];
    if (!file) return;
    setSelectedFile(file);
    try {
      const p = await api.previewSettings(file);
      setPreview(p);
    } catch {
      setPreview(null);
    }
  };

  const handleImport = async () => {
    if (!selectedFile) return;
    setImporting(true);
    try {
      await api.importSettings(selectedFile);
    } catch {
      setImporting(false);
    }
  };

  return (
    <Card title={t("settings")} id={cardId}>
      <div className="space-y-4">
        <div className="flex gap-2">
          <Button variant="outline" size="sm" onClick={handleExport}>
            {t("exportSettings")}
          </Button>
          <Button variant="outline" size="sm" onClick={() => fileInputRef.current?.click()}>
            {t("importSettings")}
          </Button>
          <input ref={fileInputRef} type="file" accept=".tar.gz,.tgz" className="hidden" onChange={handleFileSelect} />
        </div>
        {preview && (
          <div className="rounded-lg border border-amber-500/20 bg-amber-500/5 p-3 space-y-2">
            <p className="text-xs font-medium">{t("importPreview")}</p>
            <ul className="text-xs text-muted-foreground space-y-1">
              {preview.hostname && <li>Hostname: <span className="font-mono">{preview.hostname}</span></li>}
              <li>WiFi: {preview.wifi_configured ? "✓" : "—"}</li>
              <li>SSH Keys: {preview.ssh_keys_present ? "✓" : "—"}</li>
              <li>Auth: {preview.has_auth ? "✓" : "—"}</li>
              <li>{preview.files.length} {t("files")}</li>
            </ul>
            <p className="text-xs text-amber-500">{t("importRebootWarning")}</p>
            <Button size="sm" onClick={handleImport} disabled={importing}>
              {importing ? t("importing") : t("importConfirm")}
            </Button>
          </div>
        )}
      </div>
    </Card>
  );
}

function InfoTooltip({ content }: { content: string }) {
  const [isOpen, setIsOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const handleOutsideClick = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) {
        setIsOpen(false);
      }
    };
    if (isOpen) {
      document.addEventListener("click", handleOutsideClick);
    }
    return () => document.removeEventListener("click", handleOutsideClick);
  }, [isOpen]);

  return (
    <div ref={ref} className="relative inline-block ml-1.5 align-middle group">
      <button
        type="button"
        onClick={() => setIsOpen(!isOpen)}
        onMouseEnter={() => setIsOpen(true)}
        onMouseLeave={() => setIsOpen(false)}
        className="text-muted-foreground/60 hover:text-foreground transition-colors cursor-help inline-flex items-center justify-center p-0.5 rounded-sm focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
        aria-label="More info"
      >
        <svg className="size-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={2.5}>
          <path strokeLinecap="round" strokeLinejoin="round" d="M9.879 7.519c1.171-1.025 3.071-1.025 4.242 0 1.172 1.025 1.172 2.687 0 3.712-.203.179-.43.326-.67.442-.745.361-1.45.999-1.45 1.827v.75M21 12a9 9 0 11-18 0 9 9 0 0118 0zm-9 5.25h.008v.008H12v-.008z" />
        </svg>
      </button>
      {isOpen && (
        <div
          role="tooltip"
          className="absolute bottom-full left-1/2 z-50 mb-2 w-64 -translate-x-1/2 rounded-lg border border-border bg-popover p-2.5 text-left text-xs font-normal text-popover-foreground shadow-md animate-in fade-in zoom-in-95 duration-100 pointer-events-none"
        >
          {content}
          <div className="absolute top-full left-1/2 -mt-1 h-2 w-2 -translate-x-1/2 rotate-45 border-b border-r border-border bg-popover" />
        </div>
      )}
    </div>
  );
}

function HardwareTuningCard() {
  const t = useTranslations("tuning");
  const [config, setConfig] = useState<TuningConfig | null>(null);
  const [saving, setSaving] = useState(false);
  const cardId = useId();
  const wifiId = useId();
  const btId = useId();
  const onboardAudioId = useId();
  const exclusiveCoreId = useId();

  const fetchConfig = useCallback(() => {
    api.getTuning().then(setConfig).catch(() => {});
  }, []);

  useEffect(() => {
    fetchConfig();
  }, [fetchConfig]);

  useWebSocket("system_changed", fetchConfig);

  if (!config) return <Skeleton className="h-40 w-full" />;

  const toggle = async (key: keyof TuningConfig, val: boolean) => {
    const newConfig = { ...config, [key]: val };
    setConfig(newConfig);
    setSaving(true);
    try {
      await api.setTuning(newConfig);
    } catch (e) {
      console.error(e);
      setConfig(config);
    } finally {
      setSaving(false);
    }
  };

  return (
    <Card title={t("title")} id={cardId}>
      <div className="space-y-4">
        {saving && (
          <div className="text-xs text-muted-foreground animate-pulse mb-2">
            {t("saving")}
          </div>
        )}
        <div className="flex items-center justify-between">
          <div className="flex items-center">
            <label htmlFor={wifiId} className="text-sm font-medium">{t("disableWifi")}</label>
            <InfoTooltip content={t("disableWifiDesc")} />
          </div>
          <Switch id={wifiId} checked={config.rf_kill_wifi} onCheckedChange={(c) => toggle("rf_kill_wifi", c)} disabled={saving} />
        </div>
        <div className="flex items-center justify-between border-t border-border pt-4">
          <div className="flex items-center">
            <label htmlFor={btId} className="text-sm font-medium">{t("disableBluetooth")}</label>
            <InfoTooltip content={t("disableBluetoothDesc")} />
          </div>
          <Switch id={btId} checked={config.rf_kill_bluetooth} onCheckedChange={(c) => toggle("rf_kill_bluetooth", c)} disabled={saving} />
        </div>
        <div className="flex items-center justify-between border-t border-border pt-4">
          <div className="flex items-center">
            <label htmlFor={onboardAudioId} className="text-sm font-medium">{t("disableOnboardAudio")}</label>
            <InfoTooltip content={t("disableOnboardAudioDesc")} />
          </div>
          <Switch id={onboardAudioId} checked={config.disable_onboard_audio} onCheckedChange={(c) => toggle("disable_onboard_audio", c)} disabled={saving} />
        </div>
        <div className="flex items-center justify-between border-t border-border pt-4">
          <div className="flex items-center">
            <label htmlFor={exclusiveCoreId} className="text-sm font-medium">{t("exclusiveCore")}</label>
            <InfoTooltip content={t("exclusiveCoreDesc")} />
          </div>
          <Switch id={exclusiveCoreId} checked={config.exclusive_audio_core} onCheckedChange={(c) => toggle("exclusive_audio_core", c)} disabled={saving} />
        </div>
      </div>
    </Card>
  );
}

function SystemTab() {
  const t = useTranslations("system");
  const [info, setInfo] = useState<SystemInfo | null>(null);
  const [hostname, setHostname] = useState("");
  const cardId = useId();
  const hostnameId = useId();

  useEffect(() => {
    api.getSystem().then((s) => { setInfo(s); setHostname(s.hostname); }).catch(() => {});
  }, []);

  if (!info) return <Skeleton className="h-32 w-full" aria-label={t("loading")} />;

  const saveHostname = () => {
    const h = hostname.trim();
    if (h && h !== info.hostname) {
      api.setSystem({ hostname: h }).catch(() => {});
      setInfo({ ...info, hostname: h });
    } else {
      setHostname(info.hostname); // revert empty/unchanged edits
    }
  };

  return (
    <div className="space-y-5">
      <DevicePasswordCard />
      <SettingsCard />
      <TimezoneCard />
      <LogsCard />
      <HardwareTuningCard />
      <Card title={t("title")} id={cardId}>
        <div className="space-y-4">
          <Field label={t("hostname")} htmlFor={hostnameId}>
            <Input
              id={hostnameId}
              value={hostname}
              onChange={(e) => setHostname(e.target.value)}
              onBlur={saveHostname}
              onKeyDown={(e) => { if (e.key === "Enter") e.currentTarget.blur(); }}
              autoCapitalize="none"
              autoComplete="off"
              spellCheck={false}
            />
          </Field>
          <Field label={t("version")}>
            <p className="font-mono text-xs">{info.version}</p>
          </Field>
          <div className="space-y-3 border-t border-border pt-4">
            <Button variant="outline" size="sm" onClick={() => { if (window.confirm(t("rebootConfirm"))) api.reboot(); }}>
              {t("reboot")}
            </Button>
            <div className="rounded-lg border border-destructive/20 bg-destructive/5 p-3">
              <p className="mb-2 text-xs text-destructive">{t("factoryResetWarning")}</p>
              <Button variant="destructive" size="sm" onClick={() => { if (window.confirm(t("factoryResetConfirm"))) api.factoryReset(); }}>
                {t("factoryReset")}
              </Button>
            </div>
          </div>
        </div>
      </Card>
    </div>
  );
}


// ── Server Tab ────────────────────────────────────────────────

type ServerSubTab = "audio" | "sources" | "zones" | "integrations";

function Stepper({ value, onChange, min, max, step, suffix }: { value: number; onChange: (v: number) => void; min: number; max: number; step: number; suffix?: string }) {
  return (
    <div className="flex items-center gap-2">
      <Button variant="outline" size="icon-xs" onClick={() => onChange(Math.max(min, value - step))} disabled={value <= min}>−</Button>
      <span className="w-16 text-center text-sm font-mono">{value}{suffix}</span>
      <Button variant="outline" size="icon-xs" onClick={() => onChange(Math.min(max, value + step))} disabled={value >= max}>+</Button>
    </div>
  );
}

function ServerTab() {
  const t = useTranslations("server");
  const [status, setStatus] = useState<ServerStatus | null>(null);
  const [config, setConfig] = useState<ServerConfig | null>(null);
  const [subTab, setSubTab] = useState<ServerSubTab>("audio");
  const [saved, setSaved] = useState(false);
  const [saveError, setSaveError] = useState<string | null>(null);
  const [validationErrors, setValidationErrors] = useState<string[]>([]);
  const cardId = useId();

  useEffect(() => {
    api.getServerStatus().then(setStatus).catch(() => setStatus({ enabled: false, running: false }));
    api.getServer().then(setConfig).catch(() => setConfig(DEFAULT_SERVER_CONFIG));
  }, []);

  useWebSocket("server_changed", useCallback(() => {
    api.getServerStatus().then(setStatus).catch(() => {});
    api.getServer().then(setConfig).catch(() => {});
  }, []));

  const toggle = async (enabled: boolean) => {
    const prev = status;
    setStatus((s) => s ? { ...s, enabled, running: enabled } : { enabled, running: enabled });
    try {
      if (enabled) { await api.enableServer(); } else { await api.disableServer(); }
    } catch { setStatus(prev); }
  };

  const save = async () => {
    if (!config) return;
    const errors = collectServerValidationErrors(config, t);
    setValidationErrors(errors);
    setSaveError(null);
    if (errors.length > 0) return;

    try {
      await api.setServer(config);
      setSaved(true);
      setTimeout(() => setSaved(false), 2000);
    } catch (error) {
      setSaved(false);
      setSaveError(error instanceof Error ? error.message : t("saveFailed"));
    }
  };

  const SUB_TABS: { id: ServerSubTab; label: string }[] = [
    { id: "audio", label: t("subtabAudio") },
    { id: "sources", label: t("subtabSources") },
    { id: "zones", label: t("subtabZones") },
    { id: "integrations", label: t("subtabIntegrations") },
  ];

  if (!status || !config) return <Skeleton className="h-40 w-full" />;

  return (
    <Card title={t("title")} id={cardId}>
      <div className="space-y-4">
        <div className="flex items-center justify-between">
          <div>
            <span className="text-sm">{t("enable")}</span>
            <p className="text-xs text-muted-foreground">{t("enableDescription")}</p>
          </div>
          <Switch checked={status.enabled} onCheckedChange={toggle} aria-label={t("enable")} />
        </div>

        {status.enabled && (
          <a
            href={`http://${typeof window !== "undefined" ? window.location.hostname : "localhost"}:5555`}
            target="_blank"
            rel="noopener noreferrer"
            className="inline-flex items-center gap-1 text-xs text-primary underline-offset-4 hover:underline"
          >
            {t("openWebui")} ↗
          </a>
        )}

        {status.enabled && (
          <Field label={t("deviceName")} htmlFor={`${cardId}-name`}>
            <Input id={`${cardId}-name`} value={config.name} onChange={(e) => { const c = structuredClone(config); c.name = e.target.value; setConfig(c); }} placeholder="SnapDog" />
          </Field>
        )}


        {status.enabled && (
          <>
            <div className="flex gap-1 rounded-lg bg-muted p-1">
              {SUB_TABS.map((st) => (
                <button
                  key={st.id}
                  type="button"
                  className={`rounded-md px-2.5 py-1 text-xs font-medium transition-colors ${subTab === st.id ? "bg-card text-foreground shadow-sm" : "text-muted-foreground hover:text-foreground"}`}
                  onClick={() => setSubTab(st.id)}
                >
                  {st.label}
                </button>
              ))}
            </div>

            {subTab === "audio" && <ServerAudioSubTab config={config} setConfig={setConfig} />}
            {subTab === "sources" && <ServerSourcesSubTab config={config} setConfig={setConfig} />}
            {subTab === "zones" && <ServerZonesSubTab config={config} setConfig={setConfig} />}
            {subTab === "integrations" && <ServerIntegrationsSubTab config={config} setConfig={setConfig} />}

            {(validationErrors.length > 0 || saveError) && (
              <div className="rounded-2xl border border-destructive/20 bg-destructive/5 px-3 py-2 text-xs text-destructive" role="alert">
                <p className="font-medium">{saveError ?? t("validationFailed")}</p>
                {validationErrors.length > 0 && (
                  <ul className="mt-1 space-y-0.5">
                    {validationErrors.slice(0, 6).map((error) => <li key={error}>{error}</li>)}
                  </ul>
                )}
              </div>
            )}

            <div className="flex items-center gap-3 border-t border-border pt-3">
              <Button size="sm" onClick={save}>{t("save")}</Button>
              {saved && <span className="text-xs text-green-600">{t("saved")}</span>}
            </div>
          </>
        )}
      </div>
    </Card>
  );
}

function ServerAudioSubTab({ config, setConfig }: { config: ServerConfig; setConfig: (c: ServerConfig) => void }) {
  const t = useTranslations("server");
  const nameId = useId();
  const portId = useId();
  const codecId = useId();
  const pskId = useId();
  const sampleRateId = useId();
  const bitDepthId = useId();
  const sourceConflictId = useId();
  const groupVolumeId = useId();
  const unknownClientsId = useId();
  const defaultZoneId = useId();
  const logLevelId = useId();

  const update = (path: string, value: unknown) => {
    const c = structuredClone(config);
    const parts = path.split(".");
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    let obj: any = c;
    for (let i = 0; i < parts.length - 1; i++) obj = obj[parts[i]];
    obj[parts[parts.length - 1]] = value;
    setConfig(c);
  };

  return (
    <div className="space-y-3">
      <Field label={t("name")} htmlFor={nameId}>
        <Input id={nameId} value={config.snapcast.mdns_name} onChange={(e) => update("snapcast.mdns_name", e.target.value)} />
      </Field>
      <Field label={t("port")} htmlFor={portId}>
        <Input id={portId} type="number" value={config.snapcast.streaming_port} onChange={(e) => update("snapcast.streaming_port", Number(e.target.value))} />
      </Field>
      <Field label={t("codec")} htmlFor={codecId}>
        <Select id={codecId} value={config.snapcast.codec} onChange={(e) => {
          const codec = e.target.value;
          const c = structuredClone(config);
          (c.snapcast as Record<string, unknown>).codec = codec;
          if (codec === "flac" && c.audio.bit_depth > 24) c.audio.bit_depth = 24;
          if (codec.startsWith("f32")) c.audio.bit_depth = 32;
          setConfig(c);
        }}>
          <option value="PCM">PCM</option>
          <option value="FLAC">FLAC</option>
          <option value="f32lz4">f32lz4</option>
          <option value="f32lz4e">f32lz4e</option>
        </Select>
      </Field>
      {config.snapcast.codec === "f32lz4e" && (
        <Field label={t("psk")} htmlFor={pskId}>
          <Input id={pskId} value={config.snapcast.encryption_psk ?? ""} onChange={(e) => update("snapcast.encryption_psk", e.target.value || null)} />
        </Field>
      )}
      <Field label={t("sampleRate")} htmlFor={sampleRateId}>
        <Select id={sampleRateId} value={String(config.audio.sample_rate)} onChange={(e) => update("audio.sample_rate", Number(e.target.value))}>
          <option value="44100">44100</option>
          <option value="48000">48000</option>
          <option value="96000">96000</option>
        </Select>
      </Field>
      <Field label={t("bitDepth")} htmlFor={bitDepthId}>
        {config.snapcast.codec.startsWith("f32") ? (
          <Select id={bitDepthId} value="32" disabled>
            <option value="32">32 (float)</option>
          </Select>
        ) : (
          <Select id={bitDepthId} value={String(config.audio.bit_depth)} onChange={(e) => update("audio.bit_depth", Number(e.target.value))}>
            <option value="16">16</option>
            <option value="24">24</option>
            {config.snapcast.codec !== "flac" && <option value="32">32</option>}
          </Select>
        )}
      </Field>
      <Field label={t("sourceConflict")} htmlFor={sourceConflictId}>
        <Select id={sourceConflictId} value={config.audio.source_conflict} onChange={(e) => update("audio.source_conflict", e.target.value)}>
          <option value="last_wins">{t("lastWins")}</option>
          <option value="receiver_wins">{t("receiverWins")}</option>
        </Select>
      </Field>
      <Field label={t("zoneSwitchFade")}>
        <Stepper value={config.audio.zone_switch_fade_ms} onChange={(v) => update("audio.zone_switch_fade_ms", v)} min={0} max={500} step={50} suffix="ms" />
      </Field>
      <Field label={t("sourceSwitchFade")}>
        <Stepper value={config.audio.source_switch_fade_ms} onChange={(v) => update("audio.source_switch_fade_ms", v)} min={0} max={500} step={50} suffix="ms" />
      </Field>
      <Field label={t("groupVolume")} htmlFor={groupVolumeId}>
        <Select id={groupVolumeId} value={config.snapcast.group_volume_mode} onChange={(e) => update("snapcast.group_volume_mode", e.target.value)}>
          <option value="relative">{t("relative")}</option>
          <option value="absolute">{t("absolute")}</option>
        </Select>
      </Field>
      <Field label={t("unknownClients")} htmlFor={unknownClientsId}>
        <Select id={unknownClientsId} value={config.snapcast.unknown_clients} onChange={(e) => update("snapcast.unknown_clients", e.target.value)}>
          <option value="accept">{t("accept")}</option>
          <option value="ignore">{t("ignore")}</option>
          <option value="reject">{t("reject")}</option>
        </Select>
      </Field>
      <Field label={t("defaultZone")} htmlFor={defaultZoneId}>
        <Select id={defaultZoneId} value={config.snapcast.default_zone} onChange={(e) => update("snapcast.default_zone", e.target.value)}>
          {config.zones.map((z) => <option key={z.name} value={z.name}>{z.name}</option>)}
        </Select>
      </Field>
      <Field label={t("logLevel")} htmlFor={logLevelId}>
        <Select id={logLevelId} value={config.system.log_level} onChange={(e) => update("system.log_level", e.target.value)}>
          <option value="error">error</option>
          <option value="warn">warn</option>
          <option value="info">info</option>
          <option value="debug">debug</option>
        </Select>
      </Field>
      <div className="flex items-center justify-between">
        <span className="text-sm">{t("advertiseSnapcast")}</span>
        <Switch checked={config.snapcast.advertise_snapcast} onCheckedChange={(v) => update("snapcast.advertise_snapcast", v)} />
      </div>
    </div>
  );
}

function ServerSourcesSubTab({ config, setConfig }: { config: ServerConfig; setConfig: (c: ServerConfig) => void }) {
  const t = useTranslations("server");
  const subUrlId = useId();
  const subUserId = useId();
  const subPassId = useId();
  const spotNameId = useId();
  const spotBitrateId = useId();
  const airPassId = useId();

  const toggleSubsonic = (on: boolean) => {
    const c = structuredClone(config);
    c.subsonic = on ? { url: "", username: "", password: "", format: "raw" } : null;
    setConfig(c);
  };
  const toggleSpotify = (on: boolean) => {
    const c = structuredClone(config);
    c.spotify = on ? { name: "SnapDog", bitrate: 320 } : null;
    setConfig(c);
  };
  const toggleAirplay = (on: boolean) => {
    const c = structuredClone(config);
    c.airplay = on ? { password: null, mode: "airplay2" } : null;
    setConfig(c);
  };

  const updateSub = (key: string, value: string) => {
    const c = structuredClone(config);
    if (c.subsonic) (c.subsonic as Record<string, string>)[key] = value;
    setConfig(c);
  };
  const updateSpot = (key: string, value: string | number) => {
    const c = structuredClone(config);
    if (c.spotify) (c.spotify as Record<string, string | number>)[key] = value;
    setConfig(c);
  };

  const addRadio = () => {
    const c = structuredClone(config);
    c.radio.push({ name: "", url: "", cover: null });
    setConfig(c);
  };
  const removeRadio = (i: number) => {
    const c = structuredClone(config);
    c.radio.splice(i, 1);
    setConfig(c);
  };
  const updateRadio = (i: number, key: string, value: string) => {
    const c = structuredClone(config);
    (c.radio[i] as Record<string, string | null>)[key] = key === "cover" ? (value || null) : value;
    setConfig(c);
  };

  return (
    <div className="space-y-4">
      {/* Subsonic */}
      <div className="space-y-2">
        <div className="flex items-center justify-between">
          <span className="text-sm font-medium">{t("subsonic")}</span>
          <Switch checked={config.subsonic !== null} onCheckedChange={toggleSubsonic} aria-label={t("subsonic")} />
        </div>
        {config.subsonic && (
          <div className="space-y-2 pl-2 border-l-2 border-border">
            <Field label={t("url")} htmlFor={subUrlId}><Input id={subUrlId} value={config.subsonic.url} onChange={(e) => updateSub("url", e.target.value)} /></Field>
            <Field label={t("username")} htmlFor={subUserId}><Input id={subUserId} value={config.subsonic.username} onChange={(e) => updateSub("username", e.target.value)} /></Field>
            <Field label={t("password")} htmlFor={subPassId}><Input id={subPassId} type="password" value={config.subsonic.password} onChange={(e) => updateSub("password", e.target.value)} /></Field>
            <Field label={t("streamingFormat")} htmlFor={`${subPassId}-fmt`}>
              <Select id={`${subPassId}-fmt`} value={config.subsonic.format} onChange={(e) => updateSub("format", e.target.value)}>
                <option value="raw">Original (raw)</option>
                <option value="flac">FLAC</option>
                <option value="mp3">MP3</option>
                <option value="opus">Opus</option>
              </Select>
            </Field>
          </div>
        )}
      </div>
      {/* Spotify */}
      <div className="space-y-2">
        <div className="flex items-center justify-between">
          <span className="text-sm font-medium">{t("spotify")}</span>
          <Switch checked={config.spotify !== null} onCheckedChange={toggleSpotify} aria-label={t("spotify")} />
        </div>
        {config.spotify && (
          <div className="space-y-2 pl-2 border-l-2 border-border">
            <Field label={t("name")} htmlFor={spotNameId}><Input id={spotNameId} value={config.spotify.name} onChange={(e) => updateSpot("name", e.target.value)} /></Field>
            <Field label={t("bitrate")} htmlFor={spotBitrateId}>
              <Select id={spotBitrateId} value={String(config.spotify.bitrate)} onChange={(e) => updateSpot("bitrate", Number(e.target.value))}>
                <option value="96">96</option><option value="160">160</option><option value="320">320</option>
              </Select>
            </Field>
          </div>
        )}
      </div>
      {/* AirPlay */}
      <div className="space-y-2">
        <div className="flex items-center justify-between">
          <span className="text-sm font-medium">{t("airplay")}</span>
          <Switch checked={config.airplay !== null} onCheckedChange={toggleAirplay} aria-label={t("airplay")} />
        </div>
        {config.airplay && (
          <div className="space-y-2 pl-2 border-l-2 border-border">
            <Field label={t("password")} htmlFor={airPassId}><Input id={airPassId} value={config.airplay.password ?? ""} onChange={(e) => { const c = structuredClone(config); c.airplay = { ...c.airplay!, password: e.target.value || null }; setConfig(c); }} placeholder={t("airplayPasswordHint")} /></Field>
            <Field label={t("airplayMode")} htmlFor={`${airPassId}-mode`}>
              <Select id={`${airPassId}-mode`} value={config.airplay.mode} onChange={(e) => { const c = structuredClone(config); c.airplay = { ...c.airplay!, mode: e.target.value }; setConfig(c); }}>
                <option value="airplay2">AirPlay 2</option>
                <option value="airplay1">AirPlay 1 (Legacy)</option>
              </Select>
            </Field>
          </div>
        )}
      </div>
      {/* Radio */}
      <div className="space-y-2">
        <span className="text-sm font-medium">{t("radio")}</span>
        {config.radio.map((r, i) => (
          <div key={i} className="flex items-end gap-2">
            <div className="flex-1 space-y-1">
              <Input placeholder={t("stationName")} value={r.name} onChange={(e) => updateRadio(i, "name", e.target.value)} aria-label={`${t("stationName")} ${i + 1}`} />
              <Input placeholder={t("stationUrl")} value={r.url} onChange={(e) => updateRadio(i, "url", e.target.value)} aria-label={`${t("stationUrl")} ${i + 1}`} />
              <Input placeholder={t("stationCover")} value={r.cover ?? ""} onChange={(e) => updateRadio(i, "cover", e.target.value)} aria-label={`${t("stationCover")} ${i + 1}`} />
            </div>
            <Button variant="outline" size="icon-xs" onClick={() => removeRadio(i)} aria-label="Remove">×</Button>
          </div>
        ))}
        <Button variant="outline" size="xs" onClick={addRadio}>{t("addStation")}</Button>
      </div>
    </div>
  );
}

function ServerZonesSubTab({ config, setConfig }: { config: ServerConfig; setConfig: (c: ServerConfig) => void }) {
  const t = useTranslations("server");

  const addZone = () => { const c = structuredClone(config); c.zones.push({ name: "", icon: "🔊", knx: null }); setConfig(c); };
  const removeZone = (i: number) => { const c = structuredClone(config); c.zones.splice(i, 1); setConfig(c); };
  const updateZone = (i: number, key: "name" | "icon", value: string) => { const c = structuredClone(config); c.zones[i][key] = value; setConfig(c); };

  const addClient = () => { const c = structuredClone(config); c.clients.push({ name: "", mac: "", zone: config.zones[0]?.name ?? "", icon: "🔊", max_volume: 100, knx: null }); setConfig(c); };
  const removeClient = (i: number) => { const c = structuredClone(config); c.clients.splice(i, 1); setConfig(c); };
  const updateClient = (i: number, key: "name" | "mac" | "zone" | "icon" | "max_volume", value: string | number) => {
    const c = structuredClone(config);
    if (key === "max_volume") c.clients[i].max_volume = Number(value);
    else c.clients[i][key] = String(value);
    setConfig(c);
  };

  return (
    <div className="space-y-4">
      <div className="space-y-2">
        <span className="text-sm font-medium">{t("zones")}</span>
        {config.zones.map((z, i) => (
          <div key={i} className="flex items-center gap-2">
            <Input className="flex-1" placeholder={t("zoneName")} value={z.name} onChange={(e) => updateZone(i, "name", e.target.value)} aria-label={`${t("zoneName")} ${i + 1}`} />
            <Input className="w-16" placeholder={t("icon")} value={z.icon} onChange={(e) => updateZone(i, "icon", e.target.value)} aria-label={`${t("icon")} ${i + 1}`} />
            <Button variant="outline" size="icon-xs" onClick={() => removeZone(i)} aria-label="Remove">×</Button>
          </div>
        ))}
        <Button variant="outline" size="xs" onClick={addZone}>{t("addZone")}</Button>
      </div>
      <div className="space-y-2">
        <span className="text-sm font-medium">{t("clients")}</span>
        {config.clients.map((cl, i) => (
          <div key={i} className="space-y-1">
            <div className="flex items-center gap-2">
              <EmojiPicker value={cl.icon} onChange={(v) => updateClient(i, "icon", v)} />
              <Input className="flex-1" placeholder={t("clientName")} value={cl.name} onChange={(e) => updateClient(i, "name", e.target.value)} aria-label={`${t("clientName")} ${i + 1}`} />
              <Input className="w-32" placeholder={t("mac")} value={cl.mac} onChange={(e) => updateClient(i, "mac", e.target.value)} aria-label={`${t("mac")} ${i + 1}`} />
              <Select className="w-28" value={cl.zone} onChange={(e) => updateClient(i, "zone", e.target.value)} aria-label={`${t("zone")} ${i + 1}`}>
                {config.zones.map((z) => <option key={z.name} value={z.name}>{z.name}</option>)}
              </Select>
              <Button variant="outline" size="icon-xs" onClick={() => removeClient(i)} aria-label="Remove">×</Button>
            </div>
            <div className="flex items-center gap-2 pl-8">
              <span className="text-xs text-muted-foreground w-16">Max Vol</span>
              <input type="range" min={1} max={100} value={cl.max_volume} onChange={(e) => updateClient(i, "max_volume", Number(e.target.value))} className="flex-1 h-1.5 accent-primary" aria-label={`Max volume ${i + 1}`} />
              <span className="text-xs w-8 text-right">{cl.max_volume}%</span>
            </div>
          </div>
        ))}
        <Button variant="outline" size="xs" onClick={addClient}>{t("addClient")}</Button>
      </div>
    </div>
  );
}

function getConfiguredKnxCount(values: object | null) {
  if (!values) return 0;
  return Object.values(values as Record<string, string | null | undefined>).filter((value) => typeof value === "string" && value.trim() !== "").length;
}

function KnxAddressInput({ field, value, onChange }: { field: KnxField; value: string; onChange: (value: string | null) => void }) {
  const t = useTranslations("server");
  const id = useId();
  const invalid = !isValidKnxGroupAddress(value);

  return (
    <div className="rounded-2xl bg-background/70 p-2.5 ring-1 ring-border/60">
      <label htmlFor={id} className="mb-1.5 flex min-w-0 items-start justify-between gap-2">
        <span className="min-w-0">
          <span className="block truncate text-xs font-medium">{field.label}</span>
          <span className="block truncate font-mono text-[10px] text-muted-foreground">{field.key}</span>
        </span>
        <span className="shrink-0 rounded-full bg-muted px-2 py-0.5 text-[10px] text-muted-foreground">
          {field.direction} · DPT {field.dpt}
        </span>
      </label>
      <Input
        id={id}
        className="h-8 rounded-2xl font-mono text-xs"
        placeholder="1/2/3"
        value={value}
        onChange={(event) => onChange(normalizeKnxValue(event.target.value))}
        aria-invalid={invalid}
        aria-describedby={invalid ? `${id}-error` : undefined}
      />
      {invalid && (
        <p id={`${id}-error`} className="mt-1 text-[11px] text-destructive" role="alert">
          {t("knxInvalidGa")}
        </p>
      )}
    </div>
  );
}

function KnxAddressGrid({ fields, values, onChange }: { fields: readonly KnxField[]; values: object | null; onChange: (key: string, value: string | null) => void }) {
  const valueBag = (values ?? {}) as Record<string, string | null | undefined>;

  return (
    <div className="grid gap-2 sm:grid-cols-2 xl:grid-cols-3">
      {fields.map((field) => (
        <KnxAddressInput
          key={field.key}
          field={field}
          value={valueBag[field.key] ?? ""}
          onChange={(value) => onChange(field.key, value)}
        />
      ))}
    </div>
  );
}

function KnxObjectSection({
  title,
  configured,
  total,
  defaultOpen,
  children,
}: {
  title: string;
  configured: number;
  total: number;
  defaultOpen?: boolean;
  children: React.ReactNode;
}) {
  return (
    <details open={defaultOpen || undefined} className="group rounded-2xl border border-border/70 bg-muted/25">
      <summary className="flex cursor-pointer list-none items-center justify-between gap-3 rounded-2xl px-3 py-2 text-sm font-medium [&::-webkit-details-marker]:hidden">
        <span className="truncate">{title}</span>
        <span className="flex shrink-0 items-center gap-2">
          <span className="rounded-full bg-background/80 px-2 py-0.5 font-mono text-[11px] text-muted-foreground">
            {configured}/{total}
          </span>
          <span className="text-muted-foreground transition-transform group-open:rotate-180" aria-hidden="true">⌄</span>
        </span>
      </summary>
      <div className="px-3 pb-3">
        {children}
      </div>
    </details>
  );
}

function ServerIntegrationsSubTab({ config, setConfig }: { config: ServerConfig; setConfig: (c: ServerConfig) => void }) {
  const t = useTranslations("server");
  const mqttBrokerId = useId();
  const mqttUserId = useId();
  const mqttPassId = useId();
  const mqttTopicId = useId();
  const knxModeId = useId();
  const knxUrlId = useId();

  const toggleMqtt = (on: boolean) => {
    const c = structuredClone(config);
    c.mqtt = on ? { broker: "", username: null, password: null, base_topic: "snapdog" } : null;
    setConfig(c);
  };
  const updateMqtt = (key: string, value: string | null) => {
    const c = structuredClone(config);
    if (c.mqtt) (c.mqtt as Record<string, string | null>)[key] = value;
    setConfig(c);
  };

  const toggleKnx = (on: boolean) => {
    const c = structuredClone(config);
    c.knx = on ? { role: "client", url: null } : null;
    setConfig(c);
  };
  const updateKnx = (key: string, value: string | null) => {
    const c = structuredClone(config);
    if (c.knx) (c.knx as Record<string, unknown>)[key] = value;
    setConfig(c);
  };

  const updateZoneKnx = (index: number, key: ZoneKnxKey, value: string | null) => {
    const c = structuredClone(config);
    const zone = c.zones[index];
    const knx = { ...(zone.knx ?? {}) };
    (knx as Record<string, string | null>)[key] = value;
    zone.knx = compactKnxValues(knx);
    setConfig(c);
  };
  const updateClientKnx = (index: number, key: ClientKnxKey, value: string | null) => {
    const c = structuredClone(config);
    const client = c.clients[index];
    const knx = { ...(client.knx ?? {}) };
    (knx as Record<string, string | null>)[key] = value;
    client.knx = compactKnxValues(knx);
    setConfig(c);
  };
  const gatewayInvalid = config.knx?.role === "client" && !(config.knx.url ?? "").trim();

  return (
    <div className="space-y-4">
      {/* API Keys */}
      <div className="space-y-2">
        <span className="text-sm font-medium">{t("apiKeys")}</span>
        <p className="text-xs text-muted-foreground">Keys required to access the SnapDog server HTTP API.</p>
        {config.http.api_keys.map((key, i) => (
          <div key={i} className="flex items-center gap-2">
            <Input className="flex-1 font-mono text-xs" value={key} onChange={(e) => {
              const c = structuredClone(config);
              c.http.api_keys[i] = e.target.value;
              setConfig(c);
            }} aria-label={`API Key ${i + 1}`} />
            <Button variant="ghost" size="sm" onClick={() => {
              const c = structuredClone(config);
              c.http.api_keys.splice(i, 1);
              setConfig(c);
            }} aria-label="Remove key">✕</Button>
          </div>
        ))}
        <Button variant="outline" size="sm" onClick={() => {
          const c = structuredClone(config);
          c.http.api_keys.push("");
          setConfig(c);
        }}>+ Add Key</Button>
      </div>

      {/* MQTT */}
      <div className="space-y-2">
        <div className="flex items-center justify-between">
          <span className="text-sm font-medium">{t("mqtt")}</span>
          <Switch checked={config.mqtt !== null} onCheckedChange={toggleMqtt} aria-label={t("mqtt")} />
        </div>
        {config.mqtt && (
          <div className="space-y-2 pl-2 border-l-2 border-border">
            <Field label={t("broker")} htmlFor={mqttBrokerId}><Input id={mqttBrokerId} value={config.mqtt.broker} onChange={(e) => updateMqtt("broker", e.target.value)} /></Field>
            <Field label={t("username")} htmlFor={mqttUserId}><Input id={mqttUserId} value={config.mqtt.username ?? ""} onChange={(e) => updateMqtt("username", e.target.value || null)} /></Field>
            <Field label={t("password")} htmlFor={mqttPassId}><Input id={mqttPassId} type="password" value={config.mqtt.password ?? ""} onChange={(e) => updateMqtt("password", e.target.value || null)} /></Field>
            <Field label={t("baseTopic")} htmlFor={mqttTopicId}><Input id={mqttTopicId} value={config.mqtt.base_topic} onChange={(e) => updateMqtt("base_topic", e.target.value)} /></Field>
          </div>
        )}
      </div>
      {/* KNX */}
      <div className="space-y-2">
        <div className="flex items-center justify-between">
          <span className="text-sm font-medium">{t("knx")}</span>
          <Switch checked={config.knx !== null} onCheckedChange={toggleKnx} aria-label={t("knx")} />
        </div>
        {config.knx && (
          <div className="space-y-4 pl-2 border-l-2 border-border">
            <div className="grid gap-2 sm:grid-cols-2">
              <Field label={t("knxMode")} htmlFor={knxModeId}>
                <Select id={knxModeId} value={config.knx.role} onChange={(e) => {
                  const role = e.target.value as "client" | "device";
                  const c = structuredClone(config);
                  if (c.knx) {
                    c.knx.role = role;
                    if (role === "device") c.knx.url = null;
                  }
                  setConfig(c);
                }}>
                  <option value="client">{t("knxClient")}</option>
                  <option value="device">{t("knxDevice")}</option>
                </Select>
              </Field>
              {config.knx.role === "client" && (
                <Field label={t("gatewayUrl")} htmlFor={knxUrlId}>
                  <Input
                    id={knxUrlId}
                    value={config.knx.url ?? ""}
                    onChange={(e) => updateKnx("url", e.target.value || null)}
                    aria-invalid={gatewayInvalid}
                    aria-describedby={gatewayInvalid ? `${knxUrlId}-error` : undefined}
                  />
                  {gatewayInvalid && <p id={`${knxUrlId}-error`} className="text-xs text-destructive" role="alert">{t("knxGatewayRequired")}</p>}
                </Field>
              )}
            </div>

            <div className="space-y-2">
              <span className="text-xs font-medium text-muted-foreground">{t("knxZoneObjects")}</span>
              {config.zones.length === 0 && <p className="text-xs text-muted-foreground">{t("knxNoZones")}</p>}
              {config.zones.map((zone, index) => (
                <KnxObjectSection
                  key={`${zone.name}-${index}`}
                  title={`${zone.icon || "🔊"} ${zone.name || t("zoneName")}`}
                  configured={getConfiguredKnxCount(zone.knx)}
                  total={ZONE_KNX_FIELDS.length}
                  defaultOpen={index === 0}
                >
                  <KnxAddressGrid
                    fields={ZONE_KNX_FIELDS}
                    values={zone.knx}
                    onChange={(key, value) => updateZoneKnx(index, key as ZoneKnxKey, value)}
                  />
                </KnxObjectSection>
              ))}
            </div>

            <div className="space-y-2">
              <span className="text-xs font-medium text-muted-foreground">{t("knxClientObjects")}</span>
              {config.clients.length === 0 && <p className="text-xs text-muted-foreground">{t("knxNoClients")}</p>}
              {config.clients.map((client, index) => (
                <KnxObjectSection
                  key={`${client.mac}-${client.name}-${index}`}
                  title={`${client.icon || "🔊"} ${client.name || t("clientName")}`}
                  configured={getConfiguredKnxCount(client.knx)}
                  total={CLIENT_KNX_FIELDS.length}
                >
                  <KnxAddressGrid
                    fields={CLIENT_KNX_FIELDS}
                    values={client.knx}
                    onChange={(key, value) => updateClientKnx(index, key as ClientKnxKey, value)}
                  />
                </KnxObjectSection>
              ))}
            </div>
          </div>
        )}
      </div>
    </div>
  );
}

// ── Main Page ─────────────────────────────────────────────────

const TABS: Tab[] = ["dashboard", "network", "audio", "client", "server", "ssh", "update", "system"];

export default function Page() {
  const [authState, setAuthState] = useState<"loading" | "login" | "ready">("loading");
  const [loginError, setLoginError] = useState(false);
  const [password, setPassword] = useState("");
  const passwordId = useId();

  useEffect(() => {
    let cancelled = false;
    api.getAuthStatus().then((status) => {
      if (!cancelled) setAuthState(!status.enabled || status.authenticated ? "ready" : "login");
    }).catch(() => {
      if (!cancelled) setAuthState("ready");
    });
    return () => { cancelled = true; };
  }, []);

  useEffect(() => {
    const handler = () => setAuthState("login");
    window.addEventListener("snapdog-auth-expired", handler);
    return () => window.removeEventListener("snapdog-auth-expired", handler);
  }, []);

  const handleLogin = async (e: React.FormEvent) => {
    e.preventDefault();
    setLoginError(false);
    const ok = await api.login(password);
    if (ok) {
      setPassword("");
      setAuthState("ready");
    } else {
      setLoginError(true);
    }
  };

  if (authState === "loading") {
    return (
      <div className="flex items-center justify-center min-h-screen">
        <Skeleton className="h-8 w-48" />
      </div>
    );
  }

  if (authState === "login") {
    return (
      <div className="flex items-center justify-center min-h-screen p-4">
        <form onSubmit={handleLogin} className="w-full max-w-sm space-y-4">
          <div className="text-center space-y-2">
            <h1 className="text-2xl font-bold">SnapDog</h1>
            <p className="text-sm text-muted-foreground">Enter password to continue</p>
          </div>
          <div className="space-y-2">
            <label htmlFor={passwordId} className="sr-only">Password</label>
            <Input
              id={passwordId}
              type="password"
              value={password}
              onChange={(e) => { setPassword(e.target.value); setLoginError(false); }}
              placeholder="Password"
              autoFocus
              aria-invalid={loginError}
              aria-describedby={loginError ? `${passwordId}-error` : undefined}
            />
            {loginError && (
              <p id={`${passwordId}-error`} className="text-sm text-destructive" role="alert">
                Incorrect password
              </p>
            )}
          </div>
          <Button type="submit" className="w-full" disabled={!password}>
            Login
          </Button>
        </form>
      </div>
    );
  }

  return <SetupPage />;
}

function HealthBanner() {
  const t = useTranslations("health");
  const [warnings, setWarnings] = useState<{ id: string; severity: string }[]>([]);
  const [dismissed, setDismissed] = useState<Set<string>>(() => {
    if (typeof window === "undefined") return new Set();
    const stored = localStorage.getItem("snapdog_dismissed_warnings");
    return stored ? new Set(JSON.parse(stored)) : new Set();
  });

  useEffect(() => {
    api.getHealth().then((h) => setWarnings(h.warnings)).catch(() => {});
  }, []);

  const dismiss = (id: string) => {
    const next = new Set([...dismissed, id]);
    setDismissed(next);
    localStorage.setItem("snapdog_dismissed_warnings", JSON.stringify([...next]));
  };

  const critical = warnings.filter((w) => w.severity === "critical");
  const nonCritical = warnings.filter((w) => w.severity !== "critical" && !dismissed.has(w.id));

  // Critical errors: full-screen overlay
  if (critical.length > 0) {
    return (
      <div className="fixed inset-0 z-50 flex items-center justify-center bg-background/95 p-6">
        <div className="w-full max-w-md space-y-4 text-center">
          <div className="text-4xl">⚠️</div>
          <h1 className="text-xl font-bold text-destructive">System Error</h1>
          <div className="space-y-2">
            {critical.map((w) => (
              <p key={w.id} className="text-sm text-destructive">{t(w.id)}</p>
            ))}
          </div>
          <p className="text-xs text-muted-foreground">The device cannot operate normally. Try rebooting or re-flashing the SD card.</p>
          <Button onClick={() => api.reboot()}>Reboot</Button>
        </div>
      </div>
    );
  }

  if (nonCritical.length === 0) return null;

  return (
    <div className="mx-auto w-full max-w-2xl px-4 pt-4 space-y-2">
      {nonCritical.map((w) => (
        <div key={w.id} className={`flex items-center justify-between rounded-lg px-3 py-2 text-xs ${w.severity === "warn" ? "bg-yellow-500/10 text-yellow-800 dark:text-yellow-300" : "bg-blue-500/10 text-blue-800 dark:text-blue-300"}`} role="alert">
          <span>{t(w.id)}</span>
          <button type="button" onClick={() => dismiss(w.id)} className="ml-2 opacity-60 hover:opacity-100" aria-label="Dismiss">✕</button>
        </div>
      ))}
    </div>
  );
}

function SetupPage() {
  const t = useTranslations("tabs");
  const systemT = useTranslations("system");
  const [tab, setTab] = useState<Tab>("dashboard");
  const { locale, setLocale } = useI18n();
  const [isConnected, setIsConnected] = useState(true);
  const [clientEnabled, setClientEnabled] = useState(false);

  useEffect(() => {
    api.getClient().then((c) => setClientEnabled(c.server_url !== "__disabled__")).catch(() => {});
  }, []);

  useEffect(() => {
    const checkConnection = async () => {
      try {
        await api.getSystem();
        setIsConnected(true);
      } catch {
        setIsConnected(false);
      }
    };

    // Initial check
    checkConnection();

    // Poll every 5 seconds
    const interval = setInterval(checkConnection, 5000);
    return () => clearInterval(interval);
  }, []);

  return (
    <>
      <a href="#main-content" className="sr-only focus:not-sr-only focus:absolute focus:left-4 focus:top-4 focus:z-50 focus:rounded-lg focus:bg-primary focus:px-4 focus:py-2 focus:text-primary-foreground">
        {t("skipToContent")}
      </a>
      <HealthBanner />
      <header className="border-b border-border">
        <div className="mx-auto flex w-full max-w-2xl items-center gap-3 px-4 py-3">
          <img src="/icon.svg" alt="" className="size-10" aria-hidden="true" />
          <h1 className="min-w-0 flex-1 truncate font-heading text-xl font-bold">{t("heading")}</h1>
          <AboutButton />
          <Select
            value={locale}
            onChange={(e) => setLocale(e.target.value as Locale)}
            aria-label={t("language")}
            className="w-auto text-xs"
          >
            {locales.map((l) => (
              <option key={l} value={l}>{l.toUpperCase()}</option>
            ))}
          </Select>
        </div>
      </header>
      <main id="main-content" className="mx-auto w-full max-w-2xl px-4 py-6">
        <nav aria-label={t("navigation")}>
          <div className="mb-6 flex gap-1 overflow-x-auto rounded-lg bg-muted p-0.5" role="tablist" aria-label={t("navigation")}>
            {TABS.map((id) => (
              <button
                key={id}
                type="button"
                role="tab"
                id={`tab-${id}`}
                aria-selected={tab === id}
                aria-controls={`panel-${id}`}
                tabIndex={tab === id ? 0 : -1}
                className={`rounded-md px-3 py-1.5 text-sm font-medium transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring ${
                  tab === id
                    ? "bg-background text-foreground shadow-sm"
                    : "text-muted-foreground hover:text-foreground"
                }`}
                onClick={() => setTab(id)}
                onKeyDown={(e) => {
                  const idx = TABS.indexOf(id);
                  if (e.key === "ArrowRight") {
                    e.preventDefault();
                    const next = TABS[(idx + 1) % TABS.length];
                    setTab(next);
                    document.getElementById(`tab-${next}`)?.focus();
                  } else if (e.key === "ArrowLeft") {
                    e.preventDefault();
                    const prev = TABS[(idx - 1 + TABS.length) % TABS.length];
                    setTab(prev);
                    document.getElementById(`tab-${prev}`)?.focus();
                  } else if (e.key === "Home") {
                    e.preventDefault();
                    setTab(TABS[0]);
                    document.getElementById(`tab-${TABS[0]}`)?.focus();
                  } else if (e.key === "End") {
                    e.preventDefault();
                    const last = TABS[TABS.length - 1];
                    setTab(last);
                    document.getElementById(`tab-${last}`)?.focus();
                  }
                }}
              >
                {t(id)}
              </button>
            ))}
          </div>
        </nav>
        <div
          role="tabpanel"
          id={`panel-${tab}`}
          aria-labelledby={`tab-${tab}`}
          tabIndex={0}
        >
          {tab === "dashboard" && <DashboardTab />}
          {tab === "network" && <NetworkTab />}
          {tab === "audio" && <AudioTab />}
          {tab === "client" && <ClientTab />}
          {tab === "server" && <ServerTab />}
          {tab === "ssh" && <SshTab />}
          {tab === "update" && <UpdateTab />}
          {tab === "system" && <SystemTab />}
        </div>
      </main>

      <MiniPlayer clientEnabled={clientEnabled} />

      {!isConnected && (
        <div 
          className="fixed inset-0 z-50 flex items-center justify-center bg-black/40 backdrop-blur-md transition-all duration-500 animate-in fade-in"
          role="alert"
          aria-live="assertive"
        >
          <div className="mx-4 w-full max-w-sm rounded-2xl border border-destructive/20 bg-background/75 p-6 shadow-2xl backdrop-blur-xl animate-in zoom-in-95 duration-300">
            <div className="flex flex-col items-center text-center">
              <div className="relative mb-4 flex size-16 items-center justify-center">
                <span className="absolute inline-flex h-full w-full animate-ping rounded-full bg-destructive/20 opacity-75" />
                <div className="relative flex size-12 items-center justify-center rounded-full bg-destructive/10 border border-destructive/30">
                  <span className="size-3.5 rounded-full bg-destructive animate-pulse" />
                </div>
              </div>
              
              <h2 className="mb-2 text-lg font-bold text-foreground tracking-tight">
                {systemT("connectionLost")}
              </h2>
              <p className="mb-6 text-sm text-muted-foreground leading-relaxed">
                {systemT("connectionLostDetail")}
              </p>
              
              <div className="flex items-center gap-2 text-xs font-medium text-destructive/80 bg-destructive/5 px-3 py-1.5 rounded-full border border-destructive/10">
                <svg className="size-3.5 animate-spin" viewBox="0 0 24 24" fill="none">
                  <circle className="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" strokeWidth="4" />
                  <path className="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z" />
                </svg>
                <span>{systemT("reconnecting")}</span>
              </div>
            </div>
          </div>
        </div>
      )}
    </>
  );
}
