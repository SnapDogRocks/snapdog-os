"use client";

import {
  cloneElement,
  isValidElement,
  useCallback,
  useEffect,
  useId,
  useMemo,
  useRef,
  useState,
} from "react";
import { useTranslations } from "next-intl";
import { ApiError, api, type ServerAction, type ServerConfig, type ServerConfigEnvelope, type ServerConfigIssue, type ServerDiagnostics, type ServerState } from "@/lib/api";
import { mergeStructuredServerConfig } from "@/lib/server-config-merge";
import { useWebSocket } from "@/hooks/useWebSocket";
import { useFocusTrap } from "@/hooks/useFocusTrap";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Select } from "@/components/ui/select";
import { Skeleton } from "@/components/ui/skeleton";
import { Switch } from "@/components/ui/switch";
import { ConfirmDialog } from "@/components/ConfirmDialog";

type SettingsSection =
  | "overview"
  | "general"
  | "zones"
  | "clients"
  | "sources"
  | "audio"
  | "integrations"
  | "advanced";

type ApplyPhase = "idle" | "validating" | "applying";
type Notice = { tone: "success" | "warning" | "error"; text: string };
type RevisionConflict = {
  rawToml: boolean;
  message: string;
  mergeBlocked?: boolean;
  conflictPaths?: string[];
};

interface ServerSetupProps {
  onDirtyChange?: (dirty: boolean) => void;
}

const DRAFT_STORAGE_KEY = "snapdog-server-draft-v2";
const DRAFT_VERSION = 2;
const MAC_PATTERN = /^([0-9a-f]{2}:){5}[0-9a-f]{2}$/i;

const ZONE_KNX_KEYS = [
  "play", "pause", "stop", "track_next", "track_previous", "control_status",
  "volume", "volume_status", "volume_dim", "mute", "mute_status", "mute_toggle",
  "track_title_status", "track_artist_status", "track_album_status",
  "track_progress_status", "track_playing_status", "track_repeat",
  "track_repeat_status", "track_repeat_toggle", "playlist", "playlist_status",
  "playlist_next", "playlist_previous", "shuffle", "shuffle_status",
  "shuffle_toggle", "repeat", "repeat_status", "repeat_toggle", "presence",
  "presence_enable", "presence_enable_status", "presence_timer_status",
] as const;

const CLIENT_KNX_KEYS = [
  "volume", "volume_status", "volume_dim", "mute", "mute_status", "mute_toggle",
  "latency", "latency_status", "zone", "zone_status", "connected_status",
] as const;

const SYSTEM_KNX_KEYS = [
  "server_online", "all_stop", "all_mute", "all_mute_status", "system_fault", "knx_time",
] as const;

function cloneConfig(config: ServerConfig): ServerConfig {
  return structuredClone(config);
}

function defaultServerConfig(revision = "", rawToml = ""): ServerConfig {
  return {
    revision,
    raw_toml: rawToml,
    raw_toml_changed: false,
    name: "SnapDog",
    http: {
      port: 5555,
      bind: "::",
      base_url: "http://localhost:5555",
      tls_cert: null,
      tls_key: null,
      api_keys: [],
      api_docs: true,
    },
    audio: {
      sample_rate: 48000,
      bit_depth: 16,
      channels: 2,
      source_conflict: "last_wins",
      zone_switch_fade_ms: 300,
      source_switch_fade_ms: 300,
    },
    snapcast: {
      address: "127.0.0.1",
      jsonrpc_tcp_port: 1705,
      streaming_port: 1704,
      managed: true,
      verbose: false,
      codec: "flac",
      encryption_psk: null,
      group_volume_mode: "relative",
      unknown_clients: "accept",
      default_zone: "Default Zone",
    },
    mdns: { enabled: true, advertise_snapcast: false },
    dbus: { enabled: true },
    subsonic: null,
    spotify: null,
    airplay: { password: null, mode: "airplay2", bind: [] },
    mqtt: null,
    knx: null,
    zones: [{
      source_index: null,
      name: "Default Zone",
      icon: "🔊",
      sink: null,
      airplay_name: null,
      spotify_name: null,
      group_volume_mode: null,
      knx: null,
    }],
    clients: [],
    radio: [],
    system: { log_level: "info", log_file: null, state_dir: "/data/snapdog/state" },
  };
}

function configFromEnvelope(envelope: ServerConfigEnvelope): ServerConfig {
  const config = envelope.config
    ? cloneConfig(envelope.config)
    : defaultServerConfig(envelope.revision, envelope.raw_toml);
  config.revision = envelope.revision || config.revision;
  config.raw_toml = envelope.raw_toml || config.raw_toml;
  config.raw_toml_changed = false;
  config.airplay ??= { password: null, mode: "airplay2", bind: [] };
  if (envelope.state === "missing" && config.zones.length === 0) {
    config.zones = defaultServerConfig().zones;
    config.snapcast.default_zone = config.zones[0].name;
  }
  return config;
}

function comparableConfig(config: ServerConfig | null): string {
  if (!config) return "";
  return JSON.stringify(config);
}

type ConfigChange = { path: string; before: unknown; after: unknown; sensitive: boolean };

function isSensitiveConfigPath(path: string): boolean {
  return [
    "http.api_keys",
    "snapcast.encryption_psk",
    "subsonic.password",
    "airplay.password",
    "mqtt.password",
  ].some((sensitivePath) => path === sensitivePath || path.startsWith(`${sensitivePath}.`));
}

function flattenConfig(value: unknown, path: string, result: Map<string, unknown>): void {
  if (["revision", "raw_toml", "raw_toml_changed"].includes(path) || path.endsWith(".source_index")) return;
  if (isSensitiveConfigPath(path) || value == null || typeof value !== "object") {
    if (path) result.set(path, value);
    return;
  }
  if (Array.isArray(value) && value.every((item) => item == null || typeof item !== "object")) {
    result.set(path, value);
    return;
  }
  for (const [key, child] of Object.entries(value as Record<string, unknown>)) {
    flattenConfig(child, path ? `${path}.${key}` : key, result);
  }
}

function structuredConfigChanges(base: ServerConfig, draft: ServerConfig): ConfigChange[] {
  const before = new Map<string, unknown>();
  const after = new Map<string, unknown>();
  flattenConfig(base, "", before);
  flattenConfig(draft, "", after);
  return [...new Set([...before.keys(), ...after.keys()])]
    .sort((left, right) => left.localeCompare(right))
    .filter((path) => JSON.stringify(before.get(path)) !== JSON.stringify(after.get(path)))
    .map((path) => ({ path, before: before.get(path), after: after.get(path), sensitive: isSensitiveConfigPath(path) }));
}

function displayConfigValue(value: unknown): string {
  if (value == null || value === "") return "—";
  const rendered = typeof value === "string" ? value : JSON.stringify(value) ?? String(value);
  return rendered.length > 72 ? `${rendered.slice(0, 69)}…` : rendered;
}

function rawTomlLineDelta(before: string, after: string): { added: number; removed: number } {
  const beforeLines = before.split("\n");
  const afterLines = after.split("\n");
  if (beforeLines.length * afterLines.length > 1_000_000) {
    let prefix = 0;
    while (prefix < beforeLines.length && prefix < afterLines.length && beforeLines[prefix] === afterLines[prefix]) prefix += 1;
    let suffix = 0;
    while (
      suffix < beforeLines.length - prefix
      && suffix < afterLines.length - prefix
      && beforeLines[beforeLines.length - 1 - suffix] === afterLines[afterLines.length - 1 - suffix]
    ) suffix += 1;
    return { added: afterLines.length - prefix - suffix, removed: beforeLines.length - prefix - suffix };
  }
  const longest = Array(afterLines.length + 1).fill(0) as number[];
  for (const beforeLine of beforeLines) {
    let diagonal = 0;
    for (let index = 1; index <= afterLines.length; index += 1) {
      const previous = longest[index];
      longest[index] = beforeLine === afterLines[index - 1]
        ? diagonal + 1
        : Math.max(longest[index], longest[index - 1]);
      diagonal = previous;
    }
  }
  const common = longest[afterLines.length];
  return { added: afterLines.length - common, removed: beforeLines.length - common };
}

function canonicalPath(path: string | null | undefined): string {
  return (path ?? "").replace(/\[(\d+)\]/g, ".$1").replace(/^config\./, "");
}

function configIssueFromApiError(error: ApiError): ServerConfigIssue | null {
  const payload = error.payload as { issue?: Partial<ServerConfigIssue> & { summary?: unknown; detail?: unknown } } | null;
  const issue = payload?.issue;
  if (!issue || typeof issue.code !== "string") return null;
  const message = typeof issue.message === "string"
    ? issue.message
    : typeof issue.detail === "string" && issue.detail
      ? issue.detail
      : typeof issue.summary === "string" && issue.summary
        ? issue.summary
        : error.message;
  return { ...issue, code: issue.code, message, severity: "error" };
}

function configIssueFromServerState(state: ServerState | null): ServerConfigIssue | null {
  const issue = state?.issue;
  if (!issue) return null;
  return {
    code: issue.code,
    message: issue.detail || issue.summary,
    summary: issue.summary,
    detail: issue.detail,
    stage: issue.stage,
    field_path: issue.field_path,
    line: issue.line,
    column: issue.column,
    severity: "error",
  };
}

interface StoredDraft {
  version: number;
  revision: string;
  draft: ServerConfig;
  sensitive_omitted: boolean;
}

function safeStoredDraft(draft: ServerConfig): StoredDraft {
  const safe = cloneConfig(draft);
  safe.raw_toml = "";
  safe.raw_toml_changed = false;
  safe.http.api_keys = [];
  safe.snapcast.encryption_psk = null;
  if (safe.subsonic) safe.subsonic.password = "";
  safe.airplay.password = null;
  if (safe.mqtt) safe.mqtt.password = null;
  return {
    version: DRAFT_VERSION,
    revision: draft.revision,
    draft: safe,
    sensitive_omitted: true,
  };
}

function restoreStoredDraft(base: ServerConfig, stored: StoredDraft): ServerConfig | null {
  if (stored.version !== DRAFT_VERSION || stored.revision !== base.revision) return null;
  const restored = cloneConfig(stored.draft);
  restored.revision = base.revision;
  restored.raw_toml = base.raw_toml;
  restored.raw_toml_changed = false;
  restored.http.api_keys = [...base.http.api_keys];
  restored.snapcast.encryption_psk = base.snapcast.encryption_psk;
  if (restored.subsonic && base.subsonic) restored.subsonic.password = base.subsonic.password;
  restored.airplay.password = base.airplay.password;
  if (restored.mqtt && base.mqtt) restored.mqtt.password = base.mqtt.password;
  return restored;
}

function removeStoredDraft(): void {
  if (typeof window === "undefined") return;
  try { localStorage.removeItem(DRAFT_STORAGE_KEY); } catch { /* Storage is best-effort. */ }
}

function persistStoredDraft(draft: ServerConfig): void {
  if (typeof window === "undefined") return;
  try { localStorage.setItem(DRAFT_STORAGE_KEY, JSON.stringify(safeStoredDraft(draft))); } catch { /* Storage is best-effort. */ }
}

function sectionForPath(path: string): SettingsSection {
  if (path === "knx" || path.startsWith("knx.") || path.includes(".knx.")) return "integrations";
  if (path.startsWith("zones")) return "zones";
  if (path.startsWith("clients")) return "clients";
  if (path.startsWith("subsonic") || path.startsWith("spotify") || path.startsWith("airplay") || path.startsWith("radio")) return "sources";
  if (path.startsWith("audio") || path.startsWith("snapcast") || path.startsWith("system")) return "audio";
  if (path.startsWith("mqtt") || path.startsWith("knx") || path.startsWith("http.api_keys")) return "integrations";
  if (path.startsWith("raw_toml")) return "advanced";
  return "general";
}

function activeSourceNames(config: ServerConfig): string[] {
  const result: string[] = [];
  result.push("AirPlay");
  if (config.spotify) result.push("Spotify");
  if (config.subsonic) result.push("Subsonic");
  if (config.radio.length > 0) result.push("Radio");
  return result;
}

function isValidKnxGroupAddress(value: string): boolean {
  if (!/^\d+\/\d+(?:\/\d+)?$/.test(value)) return false;
  const parts = value.split("/").map(Number);
  if (parts.length === 2) return parts[0] <= 31 && parts[1] <= 2047;
  return parts.length === 3 && parts[0] <= 31 && parts[1] <= 7 && parts[2] <= 255;
}

function isValidIpAddress(value: string): boolean {
  if (/^\d{1,3}(?:\.\d{1,3}){3}$/.test(value)) {
    return value.split(".").every((part) => Number(part) <= 255);
  }
  if (!value.includes(":")) return false;
  try {
    return new URL(`http://[${value}]/`).hostname.length > 0;
  } catch {
    return false;
  }
}

function browserServerEndpoint(endpoint: string | null): string | null {
  if (!endpoint || typeof window === "undefined") return endpoint;
  try {
    const url = new URL(endpoint, window.location.origin);
    if (!["http:", "https:"].includes(url.protocol) || url.username || url.password) return null;
    const host = url.hostname.replace(/^\[(.*)\]$/, "$1");
    if (["localhost", "127.0.0.1", "0.0.0.0", "::", "::1"].includes(host)) {
      url.hostname = window.location.hostname;
    }
    url.search = "";
    url.hash = "";
    return url.toString();
  } catch {
    return null;
  }
}

function isValidPort(value: number): boolean {
  return Number.isInteger(value) && value >= 1 && value <= 65535;
}

function isValidPublicBaseUrl(value: string): boolean {
  if (!value || value.trim() !== value || /[\u0000-\u001f\u007f-\u009f]/u.test(value) || value.includes("?") || value.includes("#")) {
    return false;
  }
  if (!value.startsWith("http://") && !value.startsWith("https://")) return false;
  try {
    const url = new URL(value);
    const authority = value.slice(value.indexOf("://") + 3).split("/")[0];
    return Boolean(authority) && !authority.includes("@") && Boolean(url.host) && !url.username && !url.password;
  } catch {
    return false;
  }
}

function baseUrlAfterLocalPortChange(baseUrl: string, previousPort: number, nextPort: number): string {
  if (!isValidPort(previousPort) || !isValidPort(nextPort)) return baseUrl;
  try {
    const url = new URL(baseUrl);
    const host = url.hostname.replace(/^\[(.*)\]$/, "$1");
    const localHost = ["localhost", "127.0.0.1", "0.0.0.0", "::", "::1"].includes(host);
    const effectivePort = Number(url.port || (url.protocol === "https:" ? 443 : 80));
    if (!localHost || effectivePort !== previousPort) return baseUrl;
    url.port = String(nextPort);
    return url.toString().replace(/\/$/, "");
  } catch {
    return baseUrl;
  }
}

async function copyText(text: string): Promise<void> {
  try {
    await navigator.clipboard.writeText(text);
  } catch {
    const textarea = document.createElement("textarea");
    textarea.value = text;
    textarea.style.position = "fixed";
    textarea.style.opacity = "0";
    document.body.appendChild(textarea);
    textarea.select();
    const copied = document.execCommand("copy");
    textarea.remove();
    if (!copied) throw new Error("Copy failed");
  }
}

function localValidation(config: ServerConfig, t: (key: string, values?: Record<string, string | number>) => string): ServerConfigIssue[] {
  if (config.raw_toml_changed) return [];
  const issues: ServerConfigIssue[] = [];
  const add = (field: string, code: string, message: string) => issues.push({ field_path: field, code, message, severity: "error" });
  const wholeNumberInRange = (value: number, min: number, max: number) => Number.isInteger(value) && value >= min && value <= max;

  if (!config.name.trim()) add("name", "required", t("validationNameRequired"));
  if (!isValidPort(config.http.port)) add("http.port", "invalid_port", t("validationPort"));
  if (!isValidPublicBaseUrl(config.http.base_url)) add("http.base_url", "invalid_url", t("validationPublicBaseUrl"));
  if (!wholeNumberInRange(config.snapcast.streaming_port, 1, 65535)) add("snapcast.streaming_port", "invalid_port", t("validationPort"));
  if (!wholeNumberInRange(config.snapcast.jsonrpc_tcp_port, 1, 65535)) add("snapcast.jsonrpc_tcp_port", "invalid_port", t("validationPort"));
  if (config.http.port === config.snapcast.streaming_port) add("snapcast.streaming_port", "port_conflict", t("validationPortConflict"));
  if (!wholeNumberInRange(config.audio.channels, 1, 8)) add("audio.channels", "invalid_channels", t("validationChannels"));
  if (!wholeNumberInRange(config.audio.zone_switch_fade_ms, 0, 1000)) add("audio.zone_switch_fade_ms", "invalid_number", t("validationWholeNumberRange", { min: 0, max: 1000 }));
  if (!wholeNumberInRange(config.audio.source_switch_fade_ms, 0, 1000)) add("audio.source_switch_fade_ms", "invalid_number", t("validationWholeNumberRange", { min: 0, max: 1000 }));
  if (Boolean(config.http.tls_cert) !== Boolean(config.http.tls_key)) add("http.tls_cert", "tls_pair", t("validationTlsPair"));
  if (config.zones.length === 0) add("zones", "zone_required", t("validationZoneRequired"));

  const zoneNames = new Set<string>();
  config.zones.forEach((zone, index) => {
    const name = zone.name.trim();
    if (!name) add(`zones.${index}.name`, "required", t("validationZoneName"));
    else if (zoneNames.has(name.toLocaleLowerCase())) add(`zones.${index}.name`, "duplicate", t("validationZoneDuplicate", { name }));
    else zoneNames.add(name.toLocaleLowerCase());
  });

  const clientNames = new Set<string>();
  const clientMacs = new Set<string>();
  config.clients.forEach((client, index) => {
    if (!client.name.trim()) add(`clients.${index}.name`, "required", t("validationClientName"));
    else if (clientNames.has(client.name.trim().toLocaleLowerCase())) add(`clients.${index}.name`, "duplicate", t("validationClientDuplicate"));
    else clientNames.add(client.name.trim().toLocaleLowerCase());
    if (!MAC_PATTERN.test(client.mac.trim())) add(`clients.${index}.mac`, "invalid_mac", t("validationMac"));
    else if (clientMacs.has(client.mac.trim().toLocaleLowerCase())) add(`clients.${index}.mac`, "duplicate", t("validationMacDuplicate"));
    else clientMacs.add(client.mac.trim().toLocaleLowerCase());
    if (!config.zones.some((zone) => zone.name === client.zone)) add(`clients.${index}.zone`, "unknown_zone", t("validationClientZone"));
    if (!wholeNumberInRange(client.max_volume, 0, 100)) add(`clients.${index}.max_volume`, "invalid_volume", t("validationVolume"));
    if (!wholeNumberInRange(client.default_volume, 0, 100)) add(`clients.${index}.default_volume`, "invalid_volume", t("validationVolume"));
    if (!wholeNumberInRange(client.default_latency, -2147483648, 2147483647)) add(`clients.${index}.default_latency`, "invalid_number", t("validationWholeNumberRange", { min: -2147483648, max: 2147483647 }));
  });

  if (config.snapcast.default_zone && !config.zones.some((zone) => zone.name === config.snapcast.default_zone)) {
    add("snapcast.default_zone", "unknown_zone", t("validationDefaultZone"));
  }
  config.radio.forEach((station, index) => {
    if (!station.name.trim()) add(`radio.${index}.name`, "required", t("validationStationName"));
    if (!station.url.trim()) add(`radio.${index}.url`, "required", t("validationStationUrl"));
  });
  if (config.subsonic && !config.subsonic.url.trim()) add("subsonic.url", "required", t("validationSubsonicUrl"));
  if (config.subsonic && !wholeNumberInRange(config.subsonic.cache.max_size_mb, 0, Number.MAX_SAFE_INTEGER)) add("subsonic.cache.max_size_mb", "invalid_number", t("validationNonNegativeWholeNumber"));
  config.airplay.bind.forEach((address) => {
    if (!isValidIpAddress(address)) add("airplay.bind", "invalid_ip_address", t("validationIpAddress", { address }));
  });
  if (config.mqtt && !config.mqtt.broker.trim()) add("mqtt.broker", "required", t("validationMqttBroker"));
  if (config.knx?.role === "client" && !(config.knx.url ?? "").trim()) add("knx.url", "required", t("knxGatewayRequired"));
  if (config.knx && !wholeNumberInRange(config.knx.heartbeat_minutes, 0, 65535)) add("knx.heartbeat_minutes", "invalid_number", t("validationWholeNumberRange", { min: 0, max: 65535 }));

  const validateKnxAddress = (path: string, value: string | null | undefined) => {
    if (value && !isValidKnxGroupAddress(value.trim())) {
      add(path, "invalid_knx_group_address", t("knxInvalidGaFor", { target: path, field: value }));
    }
  };
  if (config.knx) {
    SYSTEM_KNX_KEYS.forEach((key) => {
      validateKnxAddress(`knx.${key}`, config.knx?.[key]);
    });
  }
  config.zones.forEach((zone, zoneIndex) => {
    Object.entries(zone.knx ?? {}).forEach(([key, value]) => validateKnxAddress(`zones.${zoneIndex}.knx.${key}`, value));
  });
  config.clients.forEach((client, clientIndex) => {
    Object.entries(client.knx ?? {}).forEach(([key, value]) => validateKnxAddress(`clients.${clientIndex}.knx.${key}`, value));
  });

  return issues;
}

export function ServerSetup({ onDirtyChange }: ServerSetupProps) {
  const t = useTranslations("server");
  const [state, setState] = useState<ServerState | null>(null);
  const [envelope, setEnvelope] = useState<ServerConfigEnvelope | null>(null);
  const [base, setBase] = useState<ServerConfig | null>(null);
  const [draft, setDraft] = useState<ServerConfig | null>(null);
  const [section, setSection] = useState<SettingsSection>("overview");
  const [wizardStep, setWizardStep] = useState(0);
  const [setupRecoveryMode, setSetupRecoveryMode] = useState(false);
  const [issues, setIssues] = useState<ServerConfigIssue[]>([]);
  const [stateLoadError, setStateLoadError] = useState<string | null>(null);
  const [configLoadError, setConfigLoadError] = useState<string | null>(null);
  const [notice, setNotice] = useState<Notice | null>(null);
  const [revisionConflict, setRevisionConflict] = useState<RevisionConflict | null>(null);
  const [reviewOpen, setReviewOpen] = useState(false);
  const [applyPhase, setApplyPhase] = useState<ApplyPhase>("idle");
  const [actionPending, setActionPending] = useState<ServerAction | null>(null);
  const [stopConfirmOpen, setStopConfirmOpen] = useState(false);
  const [restartConfirmOpen, setRestartConfirmOpen] = useState(false);
  const [advancedConfirmOpen, setAdvancedConfirmOpen] = useState(false);
  const [diagnosticsOpen, setDiagnosticsOpen] = useState(false);
  const [diagnostics, setDiagnostics] = useState<ServerDiagnostics | null>(null);
  const [diagnosticsError, setDiagnosticsError] = useState<string | null>(null);
  const [diagnosticsLoading, setDiagnosticsLoading] = useState(false);
  const [configRefreshDeferred, setConfigRefreshDeferred] = useState(false);
  const restoredRevision = useRef<string | null>(null);
  const initialLoadStarted = useRef(false);
  const draftEditGeneration = useRef(0);
  const configRequestGeneration = useRef(0);
  const stateRequestGeneration = useRef(0);
  const focusStatusAfterReview = useRef(false);
  const focusStatusAfterActionConfirm = useRef(false);
  const focusSectionAfterConfirm = useRef<SettingsSection | null>(null);
  const reviewTrigger = useRef<HTMLElement | null>(null);
  const zoneReferenceNames = useRef<string[]>([]);

  const dirty = useMemo(() => comparableConfig(base) !== comparableConfig(draft), [base, draft]);
  const busy = !state || Boolean(state.operation) || applyPhase !== "idle" || actionPending !== null;
  const needsSetup = state?.setup_state === "needs_setup" || envelope?.state === "missing";
  const needsRepair = state?.setup_state === "needs_repair"
    || envelope?.state === "invalid"
    || envelope?.state === "unreadable";
  const requiresRawRepair = Boolean(needsRepair && (
    envelope?.state === "unreadable"
    || !envelope?.config
    || envelope.issues?.some((issue) => issue.code === "config_syntax_error")
  ));

  const loadState = useCallback(async () => {
    const requestGeneration = ++stateRequestGeneration.current;
    try {
      const nextState = await api.getServerState();
      if (requestGeneration !== stateRequestGeneration.current) return null;
      setState(nextState);
      setStateLoadError(null);
      return nextState;
    } catch (error) {
      if (requestGeneration !== stateRequestGeneration.current) return null;
      setState(null);
      setStateLoadError(error instanceof Error ? error.message : t("stateLoadFailed"));
      return null;
    }
  }, [t]);

  const installEnvelope = useCallback((nextEnvelope: ServerConfigEnvelope, allowRestore: boolean) => {
    const nextBase = configFromEnvelope(nextEnvelope);
    let nextDraft = cloneConfig(nextBase);
    if (allowRestore && typeof window !== "undefined" && restoredRevision.current !== nextBase.revision) {
      restoredRevision.current = nextBase.revision;
      try {
        const raw = localStorage.getItem(DRAFT_STORAGE_KEY);
        if (raw) {
          const restored = restoreStoredDraft(nextBase, JSON.parse(raw) as StoredDraft);
          if (restored) {
            nextDraft = restored;
            setNotice({ tone: "warning", text: t("draftRestored") });
          }
        }
      } catch {
        removeStoredDraft();
      }
    }
    draftEditGeneration.current += 1;
    setEnvelope(nextEnvelope);
    setBase(nextBase);
    setDraft(nextDraft);
    zoneReferenceNames.current = nextDraft.zones.map((zone) => zone.name);
    setIssues(nextEnvelope.issues ?? []);
  }, [t]);

  const loadConfig = useCallback(async (allowRestore = true, replaceExistingDraft = false) => {
    const requestGeneration = ++configRequestGeneration.current;
    const editGenerationAtStart = draftEditGeneration.current;
    try {
      const nextEnvelope = await api.getServerConfig();
      if (requestGeneration !== configRequestGeneration.current) return null;
      if ((!replaceExistingDraft && dirty) || draftEditGeneration.current !== editGenerationAtStart) {
        // A WebSocket refresh that started while the editor was clean must not
        // replace a draft the user began editing before the response arrived.
        setConfigLoadError(null);
        setConfigRefreshDeferred(true);
        return null;
      }
      installEnvelope(nextEnvelope, allowRestore);
      setConfigLoadError(null);
      setConfigRefreshDeferred(false);
      return nextEnvelope;
    } catch (error) {
      if (requestGeneration !== configRequestGeneration.current) return null;
      setConfigLoadError(error instanceof Error ? error.message : t("configLoadFailed"));
      setConfigRefreshDeferred(false);
      return null;
    }
  }, [dirty, installEnvelope, t]);

  const initialLoad = useCallback(async () => {
    const stateGeneration = ++stateRequestGeneration.current;
    const configGeneration = ++configRequestGeneration.current;
    const [stateResult, configResult] = await Promise.allSettled([
      api.getServerState(),
      api.getServerConfig(),
    ]);
    if (stateGeneration !== stateRequestGeneration.current) {
      // A newer state read owns the UI now.
    } else if (stateResult.status === "fulfilled") {
      setState(stateResult.value);
      setStateLoadError(null);
    } else {
      setStateLoadError(stateResult.reason instanceof Error ? stateResult.reason.message : t("stateLoadFailed"));
    }
    if (configGeneration !== configRequestGeneration.current) {
      // A newer config read owns the editor now.
    } else if (configResult.status === "fulfilled") {
      installEnvelope(configResult.value, true);
      setConfigLoadError(null);
      setConfigRefreshDeferred(false);
    } else {
      setConfigLoadError(configResult.reason instanceof Error ? configResult.reason.message : t("configLoadFailed"));
    }
  }, [installEnvelope, t]);

  useEffect(() => {
    if (initialLoadStarted.current) return;
    const timer = window.setTimeout(() => {
      if (initialLoadStarted.current) return;
      initialLoadStarted.current = true;
      void initialLoad();
    }, 0);
    return () => window.clearTimeout(timer);
  }, [initialLoad]);

  useEffect(() => {
    const transitional = Boolean(state?.operation)
      || ["starting", "restarting", "stopping"].includes(state?.runtime_state ?? "")
      || state?.health_state === "checking"
      || applyPhase !== "idle"
      || actionPending !== null
      || Boolean(stateLoadError);
    const timer = window.setTimeout(() => { void loadState(); }, transitional ? 2500 : 10000);
    return () => window.clearTimeout(timer);
  }, [actionPending, applyPhase, loadState, state?.health_state, state?.operation, state?.runtime_state, stateLoadError]);

  // A clean editor tracks changes made by another browser or API client. A
  // dirty editor deliberately refreshes state only; its base and draft stay
  // untouched so the revision guard can offer a lossless three-way rebase.
  useWebSocket("server_changed", useCallback(() => {
    void loadState();
    if (applyPhase !== "idle") return;
    setNotice(null);
    setRevisionConflict(null);
    if (!dirty) void loadConfig(false);
    else setConfigRefreshDeferred(true);
  }, [applyPhase, dirty, loadConfig, loadState]));
  useWebSocket("server_status_changed", useCallback(() => { void loadState(); }, [loadState]));

  useEffect(() => {
    if (dirty || !configRefreshDeferred) return;
    const timer = window.setTimeout(() => { void loadConfig(false); }, 0);
    return () => window.clearTimeout(timer);
  }, [configRefreshDeferred, dirty, loadConfig]);

  useEffect(() => {
    onDirtyChange?.(dirty);
    if (!dirty || !draft || typeof window === "undefined") {
      if (!dirty) removeStoredDraft();
      return;
    }
    persistStoredDraft(draft);
  }, [dirty, draft, onDirtyChange]);

  useEffect(() => {
    if (!dirty) return;
    const guard = (event: BeforeUnloadEvent) => {
      event.preventDefault();
      event.returnValue = "";
    };
    window.addEventListener("beforeunload", guard);
    return () => window.removeEventListener("beforeunload", guard);
  }, [dirty]);

  const updateDraft = useCallback((updater: (next: ServerConfig) => void) => {
    draftEditGeneration.current += 1;
    setDraft((current) => {
      if (!current) return current;
      const next = cloneConfig(current);
      updater(next);
      return next;
    });
    setIssues([]);
    setNotice(null);
  }, []);

  const discardDraft = useCallback(() => {
    if (!base) return;
    draftEditGeneration.current += 1;
    setDraft(cloneConfig(base));
    zoneReferenceNames.current = base.zones.map((zone) => zone.name);
    setIssues(envelope?.issues ?? []);
    setNotice(null);
    setSection("overview");
    setWizardStep(0);
    setSetupRecoveryMode(false);
    removeStoredDraft();
  }, [base, envelope]);

  const focusConfigPath = useCallback((candidatePath: string) => {
    const path = canonicalPath(candidatePath);
    setSection(sectionForPath(path));
    window.setTimeout(() => {
      const wrapper = document.querySelector<HTMLElement>(`[data-server-field="${CSS.escape(path)}"]`)
        ?? document.querySelector<HTMLElement>(`[data-server-field^="${CSS.escape(`${path}.`)}"]`);
      const field = wrapper?.matches("input, select, textarea, button")
        ? wrapper
        : wrapper?.querySelector<HTMLElement>("input, select, textarea, button");
      let ancestor = field?.parentElement;
      while (ancestor) {
        if (ancestor instanceof HTMLDetailsElement) ancestor.open = true;
        ancestor = ancestor.parentElement;
      }
      field?.focus();
      field?.scrollIntoView({ block: "center" });
    }, 50);
  }, []);

  const focusIssue = useCallback((issue: ServerConfigIssue, rawTomlChanged = false) => {
    const rawTomlIssue = rawTomlChanged || issue.line != null || issue.code === "config_syntax_error";
    focusConfigPath(rawTomlIssue ? "raw_toml" : canonicalPath(issue.field_path ?? issue.field));
  }, [focusConfigPath]);

  const validateDraft = useCallback(async (): Promise<boolean> => {
    if (!draft) return false;
    const editGenerationAtStart = draftEditGeneration.current;
    const draftSnapshot = cloneConfig(draft);
    setApplyPhase("validating");
    const localIssues = localValidation(draftSnapshot, t);
    if (localIssues.length > 0) {
      setIssues(localIssues);
      setApplyPhase("idle");
      focusIssue(localIssues[0]);
      return false;
    }
    try {
      const result = await api.validateServerConfig(draftSnapshot);
      if (draftEditGeneration.current !== editGenerationAtStart) {
        setApplyPhase("idle");
        setNotice({ tone: "warning", text: t("draftChangedDuringCheck") });
        return false;
      }
      setIssues(result.issues ?? []);
      if (!result.valid) {
        setApplyPhase("idle");
        const conflict = result.issues.find((issue) => issue.code === "config_revision_conflict");
        if (conflict) {
          setRevisionConflict({ rawToml: Boolean(draft.raw_toml_changed), message: conflict.message });
          setNotice(null);
        } else if (result.issues[0]) {
          focusIssue(result.issues[0], draftSnapshot.raw_toml_changed);
        }
        return false;
      }
      setApplyPhase("idle");
      return true;
    } catch (error) {
      setApplyPhase("idle");
      if (draftEditGeneration.current !== editGenerationAtStart) {
        setNotice({ tone: "warning", text: t("draftChangedDuringCheck") });
        return false;
      }
      if (error instanceof ApiError && error.status === 409) {
        setRevisionConflict({ rawToml: Boolean(draftSnapshot.raw_toml_changed), message: error.message });
        setNotice(null);
      } else {
        const configIssue = error instanceof ApiError ? configIssueFromApiError(error) : null;
        if (configIssue) {
          setIssues([configIssue]);
          focusIssue(configIssue, draftSnapshot.raw_toml_changed);
        }
        setNotice({ tone: "error", text: error instanceof Error ? error.message : t("saveFailed") });
      }
      return false;
    }
  }, [draft, focusIssue, t]);

  const openReview = useCallback(async (trigger: HTMLElement) => {
    reviewTrigger.current = trigger;
    setNotice(null);
    if (!state) {
      reviewTrigger.current = null;
      setNotice({ tone: "error", text: t("stateLoadFailed") });
      void loadState();
      return;
    }
    if (configLoadError && !base) {
      reviewTrigger.current = null;
      setNotice({ tone: "error", text: t("configLoadFailed") });
      void loadConfig();
      return;
    }
    if (await validateDraft()) setReviewOpen(true);
    else reviewTrigger.current = null;
  }, [base, configLoadError, loadConfig, loadState, state, t, validateDraft]);

  const closeReview = useCallback(() => {
    const trigger = reviewTrigger.current;
    reviewTrigger.current = null;
    setReviewOpen(false);
    window.setTimeout(() => {
      if (trigger?.isConnected) trigger.focus();
      else document.querySelector<HTMLElement>("[data-server-status-heading]")?.focus();
    }, 0);
  }, []);

  const applyDraft = useCallback(async () => {
    if (!draft) return;
    reviewTrigger.current = null;
    // Review already ran the installed validator once. Apply performs the same
    // guard again inside the serialized transaction immediately before
    // activation, so another standalone validation request here would add wait
    // time without closing any additional race.
    stateRequestGeneration.current += 1;
    setApplyPhase("validating");
    setNotice(null);
    try {
      const result = needsSetup ? await api.setupServer(draft) : await api.putServerConfig(draft);
      const finalState = result.state ?? await loadState();
      if (result.state) {
        stateRequestGeneration.current += 1;
        setState(result.state);
        setStateLoadError(null);
      }
      const stateConfigIssue = configIssueFromServerState(finalState);
      if (draft.raw_toml_changed && stateConfigIssue) {
        setIssues([stateConfigIssue]);
        focusIssue(stateConfigIssue, true);
      }
      if (finalState?.issue && finalState.issue.rollback_succeeded) {
        setNotice({ tone: "warning", text: t("rollbackSucceeded") });
      } else if (finalState?.runtime_state === "failed" || finalState?.health_state === "unhealthy") {
        if (needsSetup) setSetupRecoveryMode(true);
        setNotice({ tone: "error", text: finalState.issue?.summary || t("applyFailed") });
      } else {
        setSetupRecoveryMode(false);
        focusStatusAfterReview.current = true;
        const refreshedConfig = await loadConfig(false, true);
        if (!refreshedConfig) {
          setNotice({ tone: "warning", text: t("applySucceededReloadFailed") });
          setReviewOpen(false);
          return;
        }
        removeStoredDraft();
        setNotice({ tone: "success", text: finalState?.desired_state === "stopped" ? t("savedWhileOff") : t("applySucceeded") });
        setSection("overview");
        setWizardStep(0);
      }
      setReviewOpen(false);
    } catch (error) {
      setReviewOpen(false);
      const refreshed = await loadState();
      if (needsSetup) setSetupRecoveryMode(true);
      if (error instanceof ApiError && error.status === 409) {
        setRevisionConflict({ rawToml: Boolean(draft.raw_toml_changed), message: error.message });
        setNotice(null);
      } else {
        const configIssue = error instanceof ApiError ? configIssueFromApiError(error) : null;
        if (configIssue) {
          setIssues([configIssue]);
          focusIssue(configIssue, draft.raw_toml_changed);
        }
        if (refreshed?.issue?.rollback_succeeded) {
          // A guarded apply intentionally returns a failure when the candidate
          // failed even if recovery succeeded. Reflect the healthy rollback as
          // a warning, not as a second contradictory error banner.
          setNotice({ tone: "warning", text: t("rollbackSucceeded") });
        } else {
          setNotice({ tone: "error", text: error instanceof Error ? error.message : t("applyFailed") });
        }
      }
    } finally {
      setApplyPhase("idle");
    }
  }, [draft, focusIssue, loadConfig, loadState, needsSetup, t]);

  const rebaseStructuredDraft = useCallback(async () => {
    if (!base || !draft || draft.raw_toml_changed) return;
    let expectedEditGeneration = draftEditGeneration.current;
    try {
      const freshEnvelope = await api.getServerConfig();
      if (draftEditGeneration.current !== expectedEditGeneration) {
        setNotice({ tone: "warning", text: t("draftChangedDuringCheck") });
        return;
      }
      const freshBase = configFromEnvelope(freshEnvelope);
      const merge = mergeStructuredServerConfig(base, draft, freshBase);
      if (!merge.ok) {
        const conflictPaths = [...new Set(merge.conflicts.map((conflict) => conflict.path))];
        setRevisionConflict({
          rawToml: false,
          mergeBlocked: true,
          conflictPaths,
          message: conflictPaths.join(", "),
        });
        setNotice(null);
        return;
      }
      const merged = merge.value;
      setEnvelope(freshEnvelope);
      setBase(freshBase);
      expectedEditGeneration = ++draftEditGeneration.current;
      setDraft(merged);
      zoneReferenceNames.current = merged.zones.map((zone) => zone.name);
      const localIssues = localValidation(merged, t);
      let result: { valid: boolean; issues: ServerConfigIssue[] };
      if (localIssues.length > 0) {
        result = { valid: false, issues: localIssues };
      } else {
        try {
          result = await api.validateServerConfig(merged);
          if (draftEditGeneration.current !== expectedEditGeneration) {
            setNotice({ tone: "warning", text: t("draftChangedDuringCheck") });
            return;
          }
        } catch (error) {
          if (draftEditGeneration.current !== expectedEditGeneration) {
            setNotice({ tone: "warning", text: t("draftChangedDuringCheck") });
            return;
          }
          if (error instanceof ApiError && error.status === 409) {
            // The active revision changed again between GET and validation.
            // Keep the first merge as the user's draft, keep freshBase as its
            // merge base, and offer another safe three-way rebase.
            setRevisionConflict({ rawToml: false, message: error.message });
            setNotice(null);
            return;
          }
          setRevisionConflict(null);
          throw error;
        }
      }
      setIssues(result.issues);
      const repeatedConflict = result.issues.find((issue) => issue.code === "config_revision_conflict");
      if (repeatedConflict) {
        setRevisionConflict({ rawToml: false, message: repeatedConflict.message });
        setNotice(null);
        return;
      }
      setRevisionConflict(null);
      if (!result.valid && result.issues[0]) focusIssue(result.issues[0]);
      setNotice({ tone: result.valid ? "warning" : "error", text: result.valid ? t("revisionRebased") : t("validationFailed") });
    } catch (error) {
      if (draftEditGeneration.current !== expectedEditGeneration) {
        setNotice({ tone: "warning", text: t("draftChangedDuringCheck") });
        return;
      }
      setNotice({ tone: "error", text: error instanceof Error ? error.message : t("configLoadFailed") });
    }
  }, [base, draft, focusIssue, t]);

  const reloadAfterConflict = useCallback(async () => {
    const loaded = await loadConfig(false, true);
    if (loaded) {
      setRevisionConflict(null);
      setSection(loaded.state === "invalid" || loaded.state === "unreadable" ? "advanced" : "overview");
    }
  }, [loadConfig]);

  const copyRawConflict = useCallback(async () => {
    if (!draft) return;
    try {
      await copyText(draft.raw_toml);
      setNotice({ tone: "success", text: t("tomlCopied") });
    } catch {
      setNotice({ tone: "error", text: t("copyFailed") });
    }
  }, [draft, t]);

  const runAction = useCallback(async (action: ServerAction) => {
    stateRequestGeneration.current += 1;
    setActionPending(action);
    setNotice(null);
    try {
      const result = await api.serverAction(action);
      const finalState = result.state ?? await loadState();
      if (result.state) {
        stateRequestGeneration.current += 1;
        setState(result.state);
        setStateLoadError(null);
      }
      if (finalState?.issue) setNotice({ tone: "error", text: finalState.issue.summary });
    } catch (error) {
      setNotice({ tone: "error", text: error instanceof Error ? error.message : t("actionFailed") });
      await loadState();
    } finally {
      setActionPending(null);
    }
  }, [loadState, t]);

  const requestDesiredState = useCallback((running: boolean) => {
    if (running) void runAction(state?.runtime_state === "failed" ? "retry" : "start");
    else setStopConfirmOpen(true);
  }, [runAction, state?.runtime_state]);

  const showDiagnostics = useCallback(async () => {
    setDiagnosticsOpen(true);
    setDiagnosticsLoading(true);
    setDiagnosticsError(null);
    try {
      setDiagnostics(await api.getServerDiagnostics());
    } catch (error) {
      setDiagnostics(null);
      setDiagnosticsError(error instanceof Error ? error.message : t("diagnosticsFailed"));
    } finally {
      setDiagnosticsLoading(false);
    }
  }, [t]);

  const focusPendingSection = useCallback(() => {
    const pending = focusSectionAfterConfirm.current;
    if (!pending) return;
    focusSectionAfterConfirm.current = null;
    window.setTimeout(() => {
      document.querySelector<HTMLElement>(`[data-server-section-heading="${pending}"]`)?.focus();
    }, 0);
  }, []);

  const openSection = useCallback((next: SettingsSection) => {
    if (!draft) return;
    if (next === "advanced" && dirty && !draft.raw_toml_changed) {
      setAdvancedConfirmOpen(true);
      return;
    }
    if (next !== "advanced" && draft.raw_toml_changed) {
      setAdvancedConfirmOpen(true);
      return;
    }
    focusSectionAfterConfirm.current = next;
    setSection(next);
    focusPendingSection();
  }, [dirty, draft, focusPendingSection]);

  const focusServerStatus = useCallback(() => {
    window.setTimeout(() => {
      document.querySelector<HTMLElement>("[data-server-status-heading]")?.focus();
    }, 0);
  }, []);

  const focusAfterActionConfirm = useCallback(() => {
    if (!focusStatusAfterActionConfirm.current) return;
    focusStatusAfterActionConfirm.current = false;
    focusServerStatus();
  }, [focusServerStatus]);

  useEffect(() => {
    if (reviewOpen || !focusStatusAfterReview.current) return;
    focusStatusAfterReview.current = false;
    const timer = window.setTimeout(focusServerStatus, 0);
    return () => window.clearTimeout(timer);
  }, [focusServerStatus, reviewOpen]);

  if (!state && !draft && !stateLoadError && !configLoadError) return <ServerSetupSkeleton />;

  return (
    <div className="space-y-4">
      <ServerStatusHeader
        state={state}
        config={base ?? draft}
        busy={busy}
        onDesiredStateChange={requestDesiredState}
        onRetry={() => void runAction("retry")}
        onDiagnostics={() => void showDiagnostics()}
        t={t}
      />

      {stateLoadError && (
        <InlineNotice tone="error">
          <p className="font-medium">{t("stateLoadFailed")}</p>
          <p className="mt-1 text-xs opacity-90">{stateLoadError}</p>
          <Button className="mt-3" size="sm" variant="outline" onClick={() => void loadState()}>{t("reload")}</Button>
        </InlineNotice>
      )}

      {configLoadError && (
        <InlineNotice tone="error">
          <p className="font-medium">{t("configLoadFailed")}</p>
          <p className="mt-1 text-xs opacity-90">{configLoadError}</p>
          <Button className="mt-3" size="sm" variant="outline" disabled={dirty} onClick={() => void loadConfig()}>{t("reload")}</Button>
        </InlineNotice>
      )}

      {state?.issue && (
        <ServerIssueCard
          state={state}
          onRepair={() => {
            if (needsSetup) setSetupRecoveryMode(true);
            if (state.issue?.rollback_succeeded) openSection("overview");
            else if (requiresRawRepair || !state.issue?.field_path) openSection("advanced");
            else focusConfigPath(state.issue.field_path);
          }}
          onRetry={() => void runAction(state.desired_state === "stopped" ? "stop" : "retry")}
          onDiagnostics={() => void showDiagnostics()}
          t={t}
        />
      )}

      {notice && <InlineNotice tone={notice.tone}>{notice.text}</InlineNotice>}

      {revisionConflict && (
        <InlineNotice tone="warning">
          <p className="font-semibold">{t("revisionConflictTitle")}</p>
          <p className="mt-1 text-xs leading-relaxed opacity-90">
            {revisionConflict.rawToml
              ? t("rawConflictDescription")
              : revisionConflict.mergeBlocked
                ? t("mergeConflictDescription")
                : t("revisionConflictDescription")}
          </p>
          {revisionConflict.mergeBlocked && revisionConflict.conflictPaths?.length ? (
            <div className="mt-2 flex flex-wrap gap-1.5">
              {revisionConflict.conflictPaths.map((path) => (
                <button
                  key={path}
                  type="button"
                  className="rounded-lg border border-current/20 bg-background/40 px-2 py-1 font-mono text-[11px] hover:bg-background/70"
                  onClick={() => focusConfigPath(path)}
                >
                  {path}
                </button>
              ))}
            </div>
          ) : (
            <p className="mt-1 font-mono text-[11px] opacity-70">{revisionConflict.message}</p>
          )}
          <div className="mt-3 flex flex-wrap gap-2">
            {revisionConflict.rawToml ? (
              <>
                <Button size="sm" variant="outline" onClick={() => void copyRawConflict()}>{t("copyToml")}</Button>
                <Button size="sm" onClick={() => void reloadAfterConflict()}>{t("reloadCurrentConfig")}</Button>
              </>
            ) : revisionConflict.mergeBlocked ? (
              <>
                <Button size="sm" variant="outline" onClick={() => void rebaseStructuredDraft()}>{t("rebaseChanges")}</Button>
                <Button size="sm" onClick={() => void reloadAfterConflict()}>{t("reloadCurrentConfig")}</Button>
              </>
            ) : (
              <Button size="sm" onClick={() => void rebaseStructuredDraft()}>{t("rebaseChanges")}</Button>
            )}
          </div>
        </InlineNotice>
      )}

      {draft && requiresRawRepair && section !== "advanced" ? (
        <RepairLanding
          envelope={envelope}
          onEdit={() => openSection("advanced")}
          onDiagnostics={() => void showDiagnostics()}
          t={t}
        />
      ) : draft && requiresRawRepair ? (
        <ExistingSetup
          config={draft}
          state={state}
          section="advanced"
          issues={issues}
          update={updateDraft}
          zoneReferenceNames={zoneReferenceNames}
          onSectionChange={openSection}
          onReview={(trigger) => void openReview(trigger)}
          onDiscard={discardDraft}
          onRestart={() => setRestartConfirmOpen(true)}
          dirty={dirty}
          busy={busy}
          t={t}
        />
      ) : draft && needsSetup && !setupRecoveryMode ? (
        <FirstRunWizard
          config={draft}
          step={wizardStep}
          issues={issues}
          onIssuesChange={setIssues}
          onStepChange={setWizardStep}
          update={updateDraft}
          zoneReferenceNames={zoneReferenceNames}
          onReview={(trigger) => void openReview(trigger)}
          t={t}
        />
      ) : draft ? (
        <ExistingSetup
          config={draft}
          state={state}
          section={section}
          issues={issues}
          update={updateDraft}
          zoneReferenceNames={zoneReferenceNames}
          onSectionChange={openSection}
          onReview={(trigger) => void openReview(trigger)}
          onDiscard={discardDraft}
          onRestart={() => setRestartConfirmOpen(true)}
          dirty={dirty}
          busy={busy}
          t={t}
        />
      ) : null}

      <ReviewSheet
        open={reviewOpen}
        base={base}
        config={draft}
        state={state}
        guidedSetup={Boolean(needsSetup && !setupRecoveryMode)}
        phase={applyPhase}
        onCancel={closeReview}
        onApply={() => void applyDraft()}
        t={t}
      />
      <DiagnosticsSheet
        open={diagnosticsOpen}
        loading={diagnosticsLoading}
        diagnostics={diagnostics}
        error={diagnosticsError}
        onRetry={() => void showDiagnostics()}
        onClose={() => setDiagnosticsOpen(false)}
        t={t}
      />
      <ConfirmDialog
        open={stopConfirmOpen}
        title={t("stopConfirmTitle")}
        description={t("stopConfirmDescription")}
        confirmLabel={t("stopServer")}
        cancelLabel={t("cancel")}
        onAfterClose={focusAfterActionConfirm}
        onCancel={() => setStopConfirmOpen(false)}
        onConfirm={() => { focusStatusAfterActionConfirm.current = true; setStopConfirmOpen(false); void runAction("stop"); }}
      />
      <ConfirmDialog
        open={restartConfirmOpen}
        title={t("restartConfirmTitle")}
        description={t("restartConfirmDescription")}
        confirmLabel={t("restartServer")}
        cancelLabel={t("cancel")}
        onAfterClose={focusAfterActionConfirm}
        onCancel={() => setRestartConfirmOpen(false)}
        onConfirm={() => { focusStatusAfterActionConfirm.current = true; setRestartConfirmOpen(false); void runAction("restart"); }}
      />
      <ConfirmDialog
        open={advancedConfirmOpen}
        title={draft?.raw_toml_changed ? t("leaveAdvancedTitle") : t("enterAdvancedTitle")}
        description={draft?.raw_toml_changed ? t("leaveAdvancedDescription") : t("enterAdvancedDescription")}
        confirmLabel={t("discardAndContinue")}
        cancelLabel={t("cancel")}
        destructive={false}
        onAfterClose={focusPendingSection}
        onCancel={() => setAdvancedConfirmOpen(false)}
        onConfirm={() => {
          setAdvancedConfirmOpen(false);
          if (!base || !draft) return;
          const leavingAdvanced = Boolean(draft.raw_toml_changed);
          focusSectionAfterConfirm.current = leavingAdvanced ? "overview" : "advanced";
          draftEditGeneration.current += 1;
          setDraft(cloneConfig(base));
          zoneReferenceNames.current = base.zones.map((zone) => zone.name);
          setIssues([]);
          setSection(leavingAdvanced ? "overview" : "advanced");
        }}
      />
    </div>
  );
}

function ServerSetupSkeleton() {
  return (
    <div className="space-y-4" aria-busy="true">
      <Skeleton className="h-44 w-full rounded-3xl" />
      <Skeleton className="h-72 w-full rounded-3xl" />
    </div>
  );
}

function Panel({ children, className = "" }: { children: React.ReactNode; className?: string }) {
  return <section className={`rounded-3xl border border-border bg-card shadow-sm ${className}`}>{children}</section>;
}

function InlineNotice({ tone, children }: { tone: Notice["tone"]; children: React.ReactNode }) {
  const classes = tone === "success"
    ? "border-green-500/25 bg-green-500/10 text-green-800 dark:text-green-200"
    : tone === "warning"
      ? "border-amber-500/25 bg-amber-500/10 text-amber-900 dark:text-amber-200"
      : "border-destructive/25 bg-destructive/10 text-destructive";
  return (
    <div
      className={`rounded-2xl border px-4 py-3 text-sm ${classes}`}
      role={tone === "error" ? "alert" : "status"}
      aria-live={tone === "error" ? "assertive" : "polite"}
    >
      {children}
    </div>
  );
}

function StatusGlyph({ tone }: { tone: "good" | "working" | "off" | "bad" }) {
  const classes = tone === "good"
    ? "bg-green-500 text-white"
    : tone === "working"
      ? "bg-primary text-primary-foreground"
      : tone === "bad"
        ? "bg-destructive text-white"
        : "bg-muted text-muted-foreground";
  return (
    <span className={`relative flex size-11 shrink-0 items-center justify-center rounded-full ${classes}`} aria-hidden="true">
      {tone === "good" ? "✓" : tone === "bad" ? "!" : tone === "working" ? <span className="size-4 animate-spin rounded-full border-2 border-current border-r-transparent motion-reduce:animate-none" /> : "○"}
    </span>
  );
}

function statePresentation(state: ServerState | null, t: (key: string) => string) {
  if (!state) return { tone: "working" as const, title: t("statusLoading"), description: t("statusLoadingDescription") };
  if (state.operation) {
    const phaseKey: Record<string, string> = {
      validating: "phaseValidating",
      staging: "phaseApplying",
      activating: "phaseApplying",
      applying: "phaseApplying",
      restarting: "phaseRestarting",
      starting: "phaseStarting",
      stopping: "phaseStopping",
      verifying: "phaseChecking",
      checking: "phaseChecking",
      health_check: "phaseChecking",
      rolling_back: "phaseRollingBack",
      recovering: "phaseRecovering",
      importing: "phaseApplying",
    };
    return { tone: "working" as const, title: t(phaseKey[state.operation.phase] ?? "statusWorking"), description: t("statusWorkingDescription") };
  }
  if (state.setup_state === "needs_setup") return { tone: "off" as const, title: t("statusNeedsSetup"), description: t("statusNeedsSetupDescription") };
  if (state.setup_state === "needs_repair" || state.config_state === "invalid" || state.config_state === "unreadable") {
    return { tone: "bad" as const, title: t("statusNeedsRepair"), description: t("statusNeedsRepairDescription") };
  }
  if (state.runtime_state === "starting" || state.runtime_state === "restarting" || state.health_state === "checking") {
    return { tone: "working" as const, title: t("statusStarting"), description: t("statusStartingDescription") };
  }
  if (state.runtime_state === "stopping") return { tone: "working" as const, title: t("statusStopping"), description: t("statusStoppingDescription") };
  if (state.runtime_state === "unknown") {
    return { tone: "bad" as const, title: t("statusUnknown"), description: t("statusUnknownDescription") };
  }
  if ((state.desired_state === "stopped" && state.runtime_state === "running")
    || (state.desired_state === "running" && state.runtime_state === "stopped")) {
    return { tone: "bad" as const, title: t("statusMismatch"), description: t("statusMismatchDescription") };
  }
  if (state.runtime_state === "running" && state.health_state === "healthy") {
    return { tone: "good" as const, title: t("statusRunning"), description: t("statusRunningDescription") };
  }
  if (state.runtime_state === "running" && ["checking", "unknown"].includes(state.health_state)) {
    return { tone: "working" as const, title: t("statusChecking"), description: t("statusCheckingDescription") };
  }
  if (state.runtime_state === "failed" || state.health_state === "unhealthy" || (state.desired_state === "running" && !state.running)) {
    return { tone: "bad" as const, title: t("statusFailed"), description: t("statusFailedDescription") };
  }
  return { tone: "off" as const, title: t("statusStopped"), description: t("statusStoppedDescription") };
}

function ServerStatusHeader({
  state,
  config,
  busy,
  onDesiredStateChange,
  onRetry,
  onDiagnostics,
  t,
}: {
  state: ServerState | null;
  config: ServerConfig | null;
  busy: boolean;
  onDesiredStateChange: (running: boolean) => void;
  onRetry: () => void;
  onDiagnostics: () => void;
  t: (key: string, values?: Record<string, string | number>) => string;
}) {
  const presentation = statePresentation(state, t);
  const desiredRunning = state?.desired_state === "running";
  const endpoint = browserServerEndpoint(state?.endpoint ?? null);
  const canOpen = state?.runtime_state === "running" && state.health_state === "healthy" && endpoint;
  const showSwitch = Boolean(state && state.setup_state !== "needs_setup");
  const mismatch = (state?.desired_state === "stopped" && state.runtime_state === "running")
    || (state?.desired_state === "running" && state.runtime_state === "stopped");
  const failed = state?.runtime_state === "failed" || state?.runtime_state === "unknown" || state?.health_state === "unhealthy" || mismatch;
  const invalidConfig = state?.setup_state === "needs_repair"
    || state?.config_state === "invalid"
    || state?.config_state === "unreadable";
  const hasIssue = Boolean(state?.issue);
  const canRetry = failed && desiredRunning && !invalidConfig && !hasIssue;
  const shouldStopUnexpectedRuntime = !hasIssue && state?.desired_state === "stopped" && state.runtime_state === "running";

  return (
    <Panel className="overflow-hidden">
      <div className="p-5 sm:p-6">
        <div className="flex items-start gap-4">
          <StatusGlyph tone={presentation.tone} />
          <div className="min-w-0 flex-1">
            <div className="flex flex-wrap items-start justify-between gap-3">
              <div>
                <p className="text-xs font-semibold uppercase tracking-[0.16em] text-muted-foreground">{t("title")}</p>
                <h2 data-server-status-heading tabIndex={-1} className="mt-1 text-xl font-semibold tracking-tight outline-none">{presentation.title}</h2>
                <p className="mt-1 max-w-lg text-sm leading-relaxed text-muted-foreground">{presentation.description}</p>
              </div>
              {showSwitch && (
                <div className="flex min-h-11 items-center gap-3 rounded-full bg-muted/70 px-3">
                  <span className="text-xs font-medium">{desiredRunning ? t("desiredOn") : t("desiredOff")}</span>
                  <Switch
                    checked={desiredRunning}
                    disabled={busy || (!desiredRunning && invalidConfig)}
                    onCheckedChange={onDesiredStateChange}
                    aria-label={t("enable")}
                    className="my-2"
                  />
                </div>
              )}
            </div>

            {config && state?.setup_state !== "needs_setup" && (
              <p className="mt-4 text-xs text-muted-foreground">
                {t("statusSummary", { zones: config.zones.length, clients: config.clients.length })}
              </p>
            )}

            <div className="mt-4 flex flex-wrap gap-2">
              {canOpen && (
                <Button asChild size="lg">
                  <a href={endpoint ?? "#"} target="_blank" rel="noopener noreferrer">{t("openWebui")} ↗</a>
                </Button>
              )}
              {canRetry && <Button size="lg" onClick={onRetry} disabled={busy}>{t("retryStart")}</Button>}
              {shouldStopUnexpectedRuntime && (
                <Button size="lg" onClick={() => onDesiredStateChange(false)} disabled={busy}>{t("stopServer")}</Button>
              )}
              {failed && !hasIssue && <Button size="lg" variant="outline" onClick={onDiagnostics}>{t("showDiagnostics")}</Button>}
            </div>
          </div>
        </div>
      </div>
      {busy && <div className="h-1 w-full overflow-hidden bg-muted"><div className="h-full w-1/2 animate-pulse rounded-full bg-primary motion-reduce:animate-none" /></div>}
    </Panel>
  );
}

function ServerIssueCard({
  state,
  onRepair,
  onRetry,
  onDiagnostics,
  t,
}: {
  state: ServerState;
  onRepair: () => void;
  onRetry: () => void;
  onDiagnostics: () => void;
  t: (key: string, values?: Record<string, string | number>) => string;
}) {
  const issue = state.issue;
  if (!issue) return null;
  const repairable = state.config_state === "invalid" || state.config_state === "unreadable" || state.setup_state === "needs_repair";
  return (
    <InlineNotice tone={issue.rollback_succeeded ? "warning" : "error"}>
      <p className="font-semibold">{issue.rollback_succeeded ? t("rollbackSucceededTitle") : issue.summary || t("statusFailed")}</p>
      {issue.detail && <p className="mt-1 text-xs leading-relaxed opacity-90">{issue.detail}</p>}
      {(issue.line || issue.field_path) && (
        <p className="mt-2 font-mono text-xs opacity-80">
          {issue.field_path ? issue.field_path : t("lineColumn", { line: issue.line ?? 0, column: issue.column ?? 0 })}
        </p>
      )}
      <div className="mt-3 flex flex-wrap gap-2">
        {(repairable || issue.rollback_succeeded) && <Button size="sm" onClick={onRepair}>{issue.rollback_succeeded ? t("reviewChanges") : t("repairConfiguration")}</Button>}
        {!repairable && !issue.rollback_succeeded && <Button size="sm" onClick={onRetry}>{state.desired_state === "stopped" ? t("stopServer") : t("retryStart")}</Button>}
        <Button size="sm" variant="outline" onClick={onDiagnostics}>{t("showDiagnostics")}</Button>
      </div>
    </InlineNotice>
  );
}

function RepairLanding({
  envelope,
  onEdit,
  onDiagnostics,
  t,
}: {
  envelope: ServerConfigEnvelope | null;
  onEdit: () => void;
  onDiagnostics: () => void;
  t: (key: string, values?: Record<string, string | number>) => string;
}) {
  const issue = envelope?.issues?.[0];
  return (
    <Panel className="p-5 sm:p-6">
      <p className="text-xs font-semibold uppercase tracking-[0.16em] text-destructive">{t("recoveryEyebrow")}</p>
      <h2 className="mt-2 text-xl font-semibold tracking-tight">{t("recoveryTitle")}</h2>
      <p className="mt-2 max-w-lg text-sm leading-relaxed text-muted-foreground">{t("recoveryDescription")}</p>
      {issue && (
        <div className="mt-4 rounded-2xl bg-destructive/10 px-4 py-3 text-sm text-destructive">
          <p>{issue.message}</p>
          {(issue.line || issue.field_path) && <p className="mt-1 font-mono text-xs">{issue.field_path ?? t("lineColumn", { line: issue.line ?? 0, column: issue.column ?? 0 })}</p>}
        </div>
      )}
      <div className="mt-5 flex flex-wrap gap-2">
        <Button size="lg" onClick={onEdit}>{t("editToml")}</Button>
        <Button size="lg" variant="outline" onClick={onDiagnostics}>{t("showDiagnostics")}</Button>
      </div>
    </Panel>
  );
}

function WizardProgress({ step, total, label }: { step: number; total: number; label: string }) {
  return (
    <div className="space-y-2" aria-label={label}>
      <div className="flex gap-1.5" aria-hidden="true">
        {Array.from({ length: total }, (_, index) => (
          <span key={index} className={`h-1.5 flex-1 rounded-full transition-colors ${index <= step ? "bg-primary" : "bg-muted"}`} />
        ))}
      </div>
      <p className="text-xs text-muted-foreground">{label}</p>
    </div>
  );
}

function FirstRunWizard({
  config,
  step,
  issues,
  onIssuesChange,
  onStepChange,
  update,
  zoneReferenceNames,
  onReview,
  t,
}: {
  config: ServerConfig;
  step: number;
  issues: ServerConfigIssue[];
  onIssuesChange: (issues: ServerConfigIssue[]) => void;
  onStepChange: (step: number) => void;
  update: (updater: (next: ServerConfig) => void) => void;
  zoneReferenceNames: { current: string[] };
  onReview: (trigger: HTMLButtonElement) => void;
  t: (key: string, values?: Record<string, string | number>) => string;
}) {
  const total = 5;
  const headingRef = useRef<HTMLHeadingElement>(null);
  const previousStep = useRef(step);
  const titleKeys = ["wizardWelcomeTitle", "wizardZonesTitle", "wizardClientsTitle", "wizardSourcesTitle", "wizardReviewTitle"];
  const descriptionKeys = ["wizardWelcomeDescription", "zoneMeaning", "wizardClientsDescription", "wizardSourcesDescription", "wizardReviewDescription"];
  const stepIssues = issues.filter((issue) => {
    const path = canonicalPath(issue.field_path ?? issue.field);
    if (step === 0) return path === "name" || path.startsWith("http");
    if (step === 1) return path.startsWith("zones") || path === "snapcast.default_zone";
    if (step === 2) return path.startsWith("clients");
    if (step === 3) return ["subsonic", "spotify", "airplay", "radio"].some((prefix) => path.startsWith(prefix));
    return true;
  });

  const next = () => {
    const currentIssues = localValidation(config, t).filter((issue) => {
      const path = canonicalPath(issue.field_path ?? issue.field);
      if (step === 0) return path === "name";
      if (step === 1) return path.startsWith("zones");
      if (step === 2) return path.startsWith("clients");
      if (step === 3) return ["subsonic", "spotify", "airplay", "radio"].some((prefix) => path.startsWith(prefix));
      return true;
    });
    if (currentIssues.length > 0) {
      onIssuesChange(currentIssues);
      const path = canonicalPath(currentIssues[0].field_path ?? currentIssues[0].field);
      window.setTimeout(() => {
        const field = document.querySelector<HTMLElement>(`[data-server-field="${CSS.escape(path)}"]`);
        field?.focus();
      }, 0);
      return;
    }
    onIssuesChange([]);
    onStepChange(Math.min(total - 1, step + 1));
  };

  useEffect(() => {
    if (previousStep.current === step) return;
    previousStep.current = step;
    headingRef.current?.focus();
  }, [step]);

  return (
    <Panel className="overflow-hidden">
      <div className="border-b border-border px-5 py-4 sm:px-6">
        <WizardProgress step={step} total={total} label={t("wizardProgress", { current: step + 1, total })} />
      </div>
      <div className="p-5 sm:p-6">
        <p className="text-xs font-semibold uppercase tracking-[0.16em] text-primary">{t("wizardEyebrow")}</p>
        <h2 ref={headingRef} tabIndex={-1} className="mt-2 text-2xl font-semibold tracking-tight outline-none">{t(titleKeys[step])}</h2>
        <p className="mt-2 max-w-xl text-sm leading-relaxed text-muted-foreground">{t(descriptionKeys[step])}</p>

        <div className="mt-6">
          {step === 0 && <GeneralEditor config={config} update={update} issues={issues} setupMode t={t} />}
          {step === 1 && <ZonesEditor config={config} update={update} issues={issues} referenceNames={zoneReferenceNames} t={t} />}
          {step === 2 && <ClientsEditor config={config} update={update} issues={issues} t={t} />}
          {step === 3 && <SourcesEditor config={config} update={update} issues={issues} compact t={t} />}
          {step === 4 && <SetupSummary config={config} t={t} />}
        </div>

        {stepIssues.length > 0 && (
          <div className="mt-5 rounded-2xl bg-destructive/10 px-4 py-3 text-sm text-destructive" role="alert">
            <p className="font-medium">{t("validationFailed")}</p>
            <ul className="mt-1 list-disc space-y-1 pl-4 text-xs">
              {stepIssues.slice(0, 5).map((issue, index) => <li key={`${issue.code}-${index}`}>{issue.message}</li>)}
            </ul>
          </div>
        )}

        <div className="mt-7 flex items-center justify-between gap-3 border-t border-border pt-5">
          <Button size="lg" variant="ghost" disabled={step === 0} onClick={() => onStepChange(Math.max(0, step - 1))}>{t("back")}</Button>
          {step < total - 1
            ? <Button size="lg" onClick={next}>{step === 2 && config.clients.length === 0 ? t("skipOrContinue") : t("continue")}</Button>
            : <Button size="lg" onClick={(event) => onReview(event.currentTarget)}>{t("reviewAndStart")}</Button>}
        </div>
      </div>
    </Panel>
  );
}

function SetupSummary({ config, t }: { config: ServerConfig; t: (key: string, values?: Record<string, string | number>) => string }) {
  const sources = activeSourceNames(config);
  return (
    <dl className="divide-y divide-border overflow-hidden rounded-2xl border border-border bg-background/50">
      <SummaryRow label={t("deviceName")} value={config.name || "—"} />
      <SummaryRow label={t("zones")} value={t("countZones", { count: config.zones.length })} />
      <SummaryRow label={t("clients")} value={t("countClients", { count: config.clients.length })} />
      <SummaryRow label={t("subtabSources")} value={sources.length ? sources.join(", ") : t("noneSelected")} />
      <SummaryRow label={t("audioAndStreaming")} value={`${config.snapcast.codec.toUpperCase()} · ${config.audio.sample_rate / 1000} kHz`} />
    </dl>
  );
}

function SummaryRow({ label, value }: { label: string; value: string }) {
  return (
    <div className="flex items-center justify-between gap-4 px-4 py-3">
      <dt className="text-sm text-muted-foreground">{label}</dt>
      <dd className="text-right text-sm font-medium">{value}</dd>
    </div>
  );
}

function ExistingSetup({
  config,
  state,
  section,
  issues,
  update,
  zoneReferenceNames,
  onSectionChange,
  onReview,
  onDiscard,
  onRestart,
  dirty,
  busy,
  t,
}: {
  config: ServerConfig;
  state: ServerState | null;
  section: SettingsSection;
  issues: ServerConfigIssue[];
  update: (updater: (next: ServerConfig) => void) => void;
  zoneReferenceNames: { current: string[] };
  onSectionChange: (section: SettingsSection) => void;
  onReview: (trigger: HTMLButtonElement) => void;
  onDiscard: () => void;
  onRestart: () => void;
  dirty: boolean;
  busy: boolean;
  t: (key: string, values?: Record<string, string | number>) => string;
}) {
  const sources = activeSourceNames(config);
  const integrations = [config.mqtt ? "MQTT" : null, config.knx ? "KNX" : null].filter(Boolean).join(", ");
  const rawMode = Boolean(config.raw_toml_changed);
  const sectionTitles: Record<Exclude<SettingsSection, "overview">, string> = {
    general: t("settingsGeneral"),
    zones: t("zones"),
    clients: t("clients"),
    sources: t("subtabSources"),
    audio: t("audioAndStreaming"),
    integrations: t("subtabIntegrations"),
    advanced: t("subtabAdvanced"),
  };

  if (section !== "overview") {
    return (
      <Panel className="overflow-hidden">
        <div className="flex items-center gap-3 border-b border-border px-4 py-3 sm:px-5">
          <Button size="icon-lg" variant="ghost" onClick={() => onSectionChange("overview")} aria-label={t("back")}>←</Button>
          <div className="min-w-0">
            <p className="text-xs font-medium text-muted-foreground">{t("title")}</p>
            <h2 data-server-section-heading={section} tabIndex={-1} className="truncate text-lg font-semibold outline-none">{sectionTitles[section]}</h2>
          </div>
        </div>
        <div className="p-5 sm:p-6">
          {section === "general" && <GeneralEditor config={config} update={update} issues={issues} t={t} />}
          {section === "zones" && <ZonesEditor config={config} update={update} issues={issues} referenceNames={zoneReferenceNames} t={t} />}
          {section === "clients" && <ClientsEditor config={config} update={update} issues={issues} t={t} />}
          {section === "sources" && <SourcesEditor config={config} update={update} issues={issues} t={t} />}
          {section === "audio" && <AudioEditor config={config} update={update} issues={issues} t={t} />}
          {section === "integrations" && <IntegrationsEditor config={config} update={update} issues={issues} t={t} />}
          {section === "advanced" && <AdvancedEditor config={config} update={update} issues={issues} t={t} />}
        </div>
        {dirty && (
          <DirtyBar
            rawMode={rawMode}
            busy={busy}
            onDiscard={onDiscard}
            onReview={onReview}
            t={t}
          />
        )}
      </Panel>
    );
  }

  return (
    <Panel className="overflow-hidden">
      <div className="flex flex-wrap items-center justify-between gap-3 border-b border-border px-5 py-4 sm:px-6">
        <div>
          <p className="text-xs font-semibold uppercase tracking-[0.16em] text-muted-foreground">{t("configuration")}</p>
          <h2 data-server-section-heading="overview" tabIndex={-1} className="mt-1 text-xl font-semibold tracking-tight outline-none">{t("settingsTitle")}</h2>
        </div>
        {state?.desired_state === "running"
          && state.runtime_state === "running"
          && state.health_state === "healthy"
          && !state.issue
          && !dirty && (
          <Button variant="outline" size="sm" disabled={busy} onClick={onRestart}>{t("restartServer")}</Button>
        )}
      </div>

      {rawMode && (
        <div className="border-b border-amber-500/20 bg-amber-500/10 px-5 py-3 text-sm text-amber-900 dark:text-amber-200">
          <p className="font-medium">{t("advancedModeActive")}</p>
          <p className="mt-0.5 text-xs opacity-80">{t("advancedModeActiveDescription")}</p>
        </div>
      )}

      <div className="divide-y divide-border">
        <SettingsRow label={t("settingsGeneral")} summary={config.name} onClick={() => onSectionChange("general")} disabled={rawMode} />
        <SettingsRow label={t("zones")} summary={t("countZones", { count: config.zones.length })} onClick={() => onSectionChange("zones")} disabled={rawMode} />
        <SettingsRow label={t("clients")} summary={t("countClients", { count: config.clients.length })} onClick={() => onSectionChange("clients")} disabled={rawMode} />
        <SettingsRow label={t("subtabSources")} summary={sources.length ? sources.join(", ") : t("noneSelected")} onClick={() => onSectionChange("sources")} disabled={rawMode} />
        <SettingsRow label={t("audioAndStreaming")} summary={`${config.snapcast.codec.toUpperCase()} · ${config.audio.sample_rate / 1000} kHz`} onClick={() => onSectionChange("audio")} disabled={rawMode} />
        <SettingsRow label={t("subtabIntegrations")} summary={integrations || t("noneSelected")} onClick={() => onSectionChange("integrations")} disabled={rawMode} />
        <SettingsRow label={t("subtabAdvanced")} summary={rawMode ? t("unsavedToml") : t("advancedSummary")} onClick={() => onSectionChange("advanced")} />
      </div>

      {issues.length > 0 && (
        <div className="border-t border-border px-5 py-4 sm:px-6">
          <InlineNotice tone="error">
            <p className="font-medium">{t("validationFailed")}</p>
            <p className="mt-1 text-xs opacity-90">{issues[0].message}</p>
          </InlineNotice>
        </div>
      )}

      {dirty && <DirtyBar rawMode={rawMode} busy={busy} onDiscard={onDiscard} onReview={onReview} t={t} />}
    </Panel>
  );
}

function SettingsRow({ label, summary, onClick, disabled = false }: { label: string; summary: string; onClick: () => void; disabled?: boolean }) {
  return (
    <button
      type="button"
      className="flex min-h-16 w-full items-center gap-4 px-5 py-3 text-left transition-colors hover:bg-muted/50 disabled:cursor-not-allowed disabled:opacity-45 sm:px-6"
      onClick={onClick}
      disabled={disabled}
    >
      <span className="min-w-0 flex-1 text-sm font-medium">{label}</span>
      <span className="max-w-[55%] truncate text-right text-sm text-muted-foreground">{summary}</span>
      <span className="text-lg text-muted-foreground" aria-hidden="true">›</span>
    </button>
  );
}

function DirtyBar({
  rawMode,
  busy,
  onDiscard,
  onReview,
  t,
}: {
  rawMode: boolean;
  busy: boolean;
  onDiscard: () => void;
  onReview: (trigger: HTMLButtonElement) => void;
  t: (key: string) => string;
}) {
  return (
    <div className="sticky bottom-0 z-10 flex flex-wrap items-center gap-3 border-t border-border bg-card/95 px-5 py-4 shadow-[0_-12px_30px_-24px_rgba(0,0,0,0.4)] backdrop-blur-xl sm:px-6">
      <p className="min-w-0 flex-1 text-sm font-medium" role="status" aria-live="polite">{rawMode ? t("tomlChangesPending") : t("changesPending")}</p>
      <Button size="lg" variant="outline" onClick={onDiscard} disabled={busy}>{t("discard")}</Button>
      <Button size="lg" onClick={(event) => onReview(event.currentTarget)} disabled={busy}>{t("reviewAndApply")}</Button>
    </div>
  );
}

function issueFor(issues: ServerConfigIssue[], path: string): ServerConfigIssue | undefined {
  return issues.find((issue) => canonicalPath(issue.field_path ?? issue.field) === path);
}

function FormField({
  label,
  path,
  issues,
  description,
  children,
}: {
  label: string;
  path: string;
  issues: ServerConfigIssue[];
  description?: string;
  children: React.ReactElement;
}) {
  const id = useId();
  const issue = issueFor(issues, path);
  const describedBy = [description ? `${id}-description` : null, issue ? `${id}-error` : null].filter(Boolean).join(" ") || undefined;
  const control = isValidElement(children)
    ? cloneElement(children, {
      id,
      "data-server-field": path,
      "aria-invalid": Boolean(issue),
      "aria-describedby": describedBy,
    } as React.HTMLAttributes<HTMLElement>)
    : children;
  return (
    <div className="space-y-1.5">
      <label htmlFor={id} className="block text-sm font-medium">{label}</label>
      {description && <p id={`${id}-description`} className="text-xs leading-relaxed text-muted-foreground">{description}</p>}
      {control}
      {issue && <p id={`${id}-error`} className="text-xs text-destructive" role="alert">{issue.message}</p>}
    </div>
  );
}

function GeneralEditor({
  config,
  update,
  issues,
  setupMode = false,
  t,
}: {
  config: ServerConfig;
  update: (updater: (next: ServerConfig) => void) => void;
  issues: ServerConfigIssue[];
  setupMode?: boolean;
  t: (key: string) => string;
}) {
  const lastValidHttpPort = useRef(config.http.port);
  useEffect(() => {
    if (isValidPort(config.http.port)) lastValidHttpPort.current = config.http.port;
  }, [config.http.port]);

  const updateHttpPort = (rawValue: string) => {
    const nextPort = rawValue === "" ? 0 : Number(rawValue);
    const previousPort = lastValidHttpPort.current;
    update((next) => {
      if (isValidPort(nextPort)) {
        next.http.base_url = baseUrlAfterLocalPortChange(next.http.base_url, previousPort, nextPort);
      }
      next.http.port = nextPort;
    });
    if (isValidPort(nextPort)) lastValidHttpPort.current = nextPort;
  };

  return (
    <div className="space-y-5">
      <FormField label={t("deviceName")} path="name" issues={issues} description={setupMode ? t("deviceNameDescription") : undefined}>
        <Input value={config.name} onChange={(event) => update((next) => { next.name = event.target.value; })} className="h-11 rounded-xl" />
      </FormField>
      {!setupMode && (
        <>
          <FormField label={t("httpPort")} path="http.port" issues={issues} description={t("httpPortDescription")}>
            <Input type="number" min={1} max={65535} value={config.http.port || ""} onChange={(event) => updateHttpPort(event.target.value)} className="h-11 rounded-xl" />
          </FormField>
          <ToggleRow label="mDNS / Bonjour" description={t("mdnsDescription")} checked={config.mdns.enabled} onChange={(checked) => update((next) => { next.mdns.enabled = checked; })} />
          <ToggleRow label={t("advertiseSnapcast")} description={t("advertiseSnapcastDescription")} checked={config.mdns.advertise_snapcast} onChange={(checked) => update((next) => { next.mdns.advertise_snapcast = checked; })} />
          <AdvancedOptions t={t}>
            <div className="grid gap-4 sm:grid-cols-2">
              <FormField label={t("bindAddress")} path="http.bind" issues={issues}>
                <Input value={config.http.bind} onChange={(event) => update((next) => { next.http.bind = event.target.value; })} className="h-11 rounded-xl font-mono" />
              </FormField>
              <FormField label={t("baseUrl")} path="http.base_url" issues={issues}>
                <Input type="url" value={config.http.base_url} onChange={(event) => update((next) => { next.http.base_url = event.target.value; })} className="h-11 rounded-xl" />
              </FormField>
              <FormField label={t("tlsCertificate")} path="http.tls_cert" issues={issues}>
                <Input value={config.http.tls_cert ?? ""} onChange={(event) => update((next) => { next.http.tls_cert = event.target.value || null; })} className="h-11 rounded-xl font-mono" />
              </FormField>
              <FormField label={t("tlsPrivateKey")} path="http.tls_key" issues={issues}>
                <Input value={config.http.tls_key ?? ""} onChange={(event) => update((next) => { next.http.tls_key = event.target.value || null; })} className="h-11 rounded-xl font-mono" />
              </FormField>
            </div>
            <ToggleRow label={t("apiDocs")} checked={config.http.api_docs} onChange={(checked) => update((next) => { next.http.api_docs = checked; })} />
            <ToggleRow label={t("dbusIntegration")} checked={config.dbus.enabled} onChange={(checked) => update((next) => { next.dbus.enabled = checked; })} />
          </AdvancedOptions>
        </>
      )}
    </div>
  );
}

function AdvancedOptions({ children, t }: { children: React.ReactNode; t: (key: string) => string }) {
  return (
    <details className="group overflow-hidden rounded-2xl border border-border bg-background/40">
      <summary className="flex min-h-12 cursor-pointer list-none items-center justify-between gap-3 px-4 py-3 text-sm font-medium marker:hidden">
        {t("advancedOptions")}
        <span className="text-lg text-muted-foreground transition-transform group-open:rotate-90" aria-hidden="true">›</span>
      </summary>
      <div className="space-y-4 border-t border-border p-4">{children}</div>
    </details>
  );
}

function ToggleRow({ label, description, checked, onChange }: { label: string; description?: string; checked: boolean; onChange: (checked: boolean) => void }) {
  return (
    <div className="flex min-h-14 items-center justify-between gap-4 rounded-2xl bg-muted/35 px-4 py-3">
      <div className="min-w-0">
        <p className="text-sm font-medium">{label}</p>
        {description && <p className="mt-0.5 text-xs leading-relaxed text-muted-foreground">{description}</p>}
      </div>
      <Switch checked={checked} onCheckedChange={onChange} aria-label={label} />
    </div>
  );
}

function ZonesEditor({ config, update, issues, referenceNames: referenceNamesRef, t }: EditorProps & { referenceNames: { current: string[] } }) {
  const [pendingDelete, setPendingDelete] = useState<number | null>(null);
  const [deleteTarget, setDeleteTarget] = useState("");
  const addZone = () => {
    const name = t("newZoneName", { number: config.zones.length + 1 });
    referenceNamesRef.current = [...referenceNamesRef.current, name];
    update((next) => {
      next.zones.push({ source_index: null, name, icon: "🔊", sink: null, airplay_name: null, spotify_name: null, group_volume_mode: null, knx: null });
      if (!next.snapcast.default_zone) next.snapcast.default_zone = name;
    });
  };

  const renameZone = (index: number, name: string) => {
    const referenceName = referenceNamesRef.current[index] ?? config.zones[index].name;
    const duplicate = config.zones.some((zone, candidate) => candidate !== index && zone.name === name);
    if (!duplicate) referenceNamesRef.current[index] = name;
    update((next) => {
      next.zones[index].name = name;
      if (duplicate) return;
      next.clients.forEach((client) => { if (client.zone === referenceName) client.zone = name; });
      if (next.snapcast.default_zone === referenceName) next.snapcast.default_zone = name;
    });
  };

  const removeZone = (index: number, fallback: string, referenceName: string) => {
    referenceNamesRef.current = referenceNamesRef.current.filter((_, candidate) => candidate !== index);
    update((next) => {
      next.zones.splice(index, 1);
      next.clients.forEach((client) => { if (client.zone === referenceName) client.zone = fallback; });
      if (next.snapcast.default_zone === referenceName) next.snapcast.default_zone = fallback || null;
    });
  };

  const requestRemoveZone = (index: number) => {
    const zone = config.zones[index];
    const referenceName = referenceNamesRef.current[index] ?? zone.name;
    const fallback = config.zones.find((_, candidate) => candidate !== index)?.name ?? "";
    const inUse = config.clients.some((client) => client.zone === referenceName) || config.snapcast.default_zone === referenceName;
    if (!inUse) {
      removeZone(index, fallback, referenceName);
      return;
    }
    setDeleteTarget(fallback);
    setPendingDelete(index);
  };

  return (
    <div className="space-y-4">
      <div className="rounded-2xl bg-primary/10 px-4 py-3 text-sm leading-relaxed text-muted-foreground">{t("zoneMeaning")}</div>
      {config.zones.map((zone, index) => (
        <div key={`${zone.source_index ?? "new"}-${index}`} className="space-y-3 rounded-2xl border border-border bg-background/40 p-4">
          <div className="flex items-start gap-3">
            <Input
              value={zone.icon}
              onChange={(event) => update((next) => { next.zones[index].icon = event.target.value; })}
              className="h-11 w-14 rounded-xl px-2 text-center text-lg"
              aria-label={t("icon")}
            />
            <div className="min-w-0 flex-1">
              <FormField label={t("zoneName")} path={`zones.${index}.name`} issues={issues}>
                <Input
                  value={zone.name}
                  onChange={(event) => renameZone(index, event.target.value)}
                  className="h-11 rounded-xl"
                />
              </FormField>
            </div>
            <Button size="icon-lg" variant="ghost" disabled={config.zones.length <= 1} onClick={() => requestRemoveZone(index)} aria-label={t("removeZone")}>×</Button>
          </div>
          <p className="text-xs text-muted-foreground">{t("zoneClientCount", { count: config.clients.filter((client) => client.zone === zone.name).length })}</p>
          <AdvancedOptions t={t}>
            <div className="grid gap-4 sm:grid-cols-2">
              <FormField label={t("sink")} path={`zones.${index}.sink`} issues={issues}>
                <Input value={zone.sink ?? ""} onChange={(event) => update((next) => { next.zones[index].sink = event.target.value || null; })} className="h-11 rounded-xl font-mono" />
              </FormField>
              <FormField label={t("groupVolume")} path={`zones.${index}.group_volume_mode`} issues={issues}>
                <Select value={zone.group_volume_mode ?? ""} onChange={(event) => update((next) => { next.zones[index].group_volume_mode = event.target.value || null; })} className="h-11 rounded-xl">
                  <option value="">{t("inherit")}</option><option value="relative">{t("relative")}</option><option value="absolute">{t("absolute")}</option><option value="compressed">{t("compressed")}</option>
                </Select>
              </FormField>
              <FormField label={t("airplayName")} path={`zones.${index}.airplay_name`} issues={issues}>
                <Input value={zone.airplay_name ?? ""} onChange={(event) => update((next) => { next.zones[index].airplay_name = event.target.value || null; })} className="h-11 rounded-xl" />
              </FormField>
              <FormField label={t("spotifyName")} path={`zones.${index}.spotify_name`} issues={issues}>
                <Input value={zone.spotify_name ?? ""} onChange={(event) => update((next) => { next.zones[index].spotify_name = event.target.value || null; })} className="h-11 rounded-xl" />
              </FormField>
            </div>
          </AdvancedOptions>
          {pendingDelete === index && (
            <div className="space-y-3 rounded-2xl border border-amber-500/25 bg-amber-500/10 p-4 text-amber-950 dark:text-amber-100" role="group" aria-label={t("removeZoneConfirmTitle")}>
              <div>
                <p className="text-sm font-semibold">{t("removeZoneConfirmTitle")}</p>
                <p className="mt-1 text-xs leading-relaxed opacity-85">{t("removeZoneConfirmDescription", { zone: zone.name })}</p>
              </div>
              <label className="block space-y-1.5 text-sm font-medium">
                <span>{t("moveClientsTo")}</span>
                <Select value={deleteTarget} onChange={(event) => setDeleteTarget(event.target.value)} className="h-11 rounded-xl bg-background text-foreground">
                  {config.zones.filter((_, candidate) => candidate !== index).map((candidate) => <option key={candidate.name} value={candidate.name}>{candidate.icon} {candidate.name}</option>)}
                </Select>
              </label>
              <div className="flex flex-wrap justify-end gap-2">
                <Button size="sm" variant="outline" onClick={() => setPendingDelete(null)}>{t("cancel")}</Button>
                <Button size="sm" disabled={!deleteTarget} onClick={() => { removeZone(index, deleteTarget, referenceNamesRef.current[index] ?? zone.name); setPendingDelete(null); }}>{t("removeZoneNow")}</Button>
              </div>
            </div>
          )}
        </div>
      ))}
      <Button size="lg" variant="outline" onClick={addZone}>＋ {t("addZone")}</Button>
    </div>
  );
}

interface EditorProps {
  config: ServerConfig;
  update: (updater: (next: ServerConfig) => void) => void;
  issues: ServerConfigIssue[];
  t: (key: string, values?: Record<string, string | number>) => string;
}

function ClientsEditor({ config, update, issues, t }: EditorProps) {
  const addClient = () => update((next) => {
    next.clients.push({
      source_index: null,
      name: "",
      mac: "",
      zone: next.zones[0]?.name ?? "",
      icon: "🔊",
      max_volume: 100,
      default_volume: 50,
      default_latency: 0,
      knx: null,
    });
  });

  return (
    <div className="space-y-4">
      <div className="rounded-2xl bg-primary/10 px-4 py-3 text-sm leading-relaxed text-muted-foreground">{t("clientsDescription")}</div>
      {config.clients.length === 0 && (
        <div className="rounded-2xl border border-dashed border-border px-5 py-8 text-center">
          <p className="font-medium">{t("noClientsTitle")}</p>
          <p className="mx-auto mt-1 max-w-sm text-sm text-muted-foreground">{t("noClientsDescription")}</p>
        </div>
      )}
      {config.clients.map((client, index) => (
        <div key={`${client.source_index ?? "new"}-${index}`} className="space-y-4 rounded-2xl border border-border bg-background/40 p-4">
          <div className="flex items-center justify-between gap-3">
            <div className="flex min-w-0 items-center gap-3">
              <span className="flex size-10 items-center justify-center rounded-full bg-muted text-lg">{client.icon || "🔊"}</span>
              <p className="truncate font-medium">{client.name || t("newClient")}</p>
            </div>
            <Button size="icon-lg" variant="ghost" onClick={() => update((next) => { next.clients.splice(index, 1); })} aria-label={t("removeClient")}>×</Button>
          </div>
          <div className="grid gap-4 sm:grid-cols-2">
            <FormField label={t("clientName")} path={`clients.${index}.name`} issues={issues}>
              <Input value={client.name} onChange={(event) => update((next) => { next.clients[index].name = event.target.value; })} className="h-11 rounded-xl" />
            </FormField>
            <FormField label={t("mac")} path={`clients.${index}.mac`} issues={issues}>
              <Input value={client.mac} onChange={(event) => update((next) => { next.clients[index].mac = event.target.value.toLowerCase(); })} className="h-11 rounded-xl font-mono" placeholder="aa:bb:cc:dd:ee:ff" />
            </FormField>
            <FormField label={t("zone")} path={`clients.${index}.zone`} issues={issues}>
              <Select value={client.zone} onChange={(event) => update((next) => { next.clients[index].zone = event.target.value; })} className="h-11 rounded-xl">
                {config.zones.map((zone) => <option key={zone.name} value={zone.name}>{zone.icon} {zone.name}</option>)}
              </Select>
            </FormField>
            <FormField label={t("icon")} path={`clients.${index}.icon`} issues={issues}>
              <Input value={client.icon} onChange={(event) => update((next) => { next.clients[index].icon = event.target.value; })} className="h-11 rounded-xl" />
            </FormField>
          </div>
          <div className="space-y-2">
            <div className="flex items-center justify-between text-sm"><span>{t("maxVolume")}</span><span>{client.max_volume}%</span></div>
            <input type="range" min={0} max={100} value={client.max_volume} onChange={(event) => update((next) => { next.clients[index].max_volume = Number(event.target.value); })} className="h-11 w-full accent-primary" aria-label={t("maxVolume")} />
          </div>
          <AdvancedOptions t={t}>
            <div className="grid gap-4 sm:grid-cols-2">
              <FormField label={t("defaultVolume")} path={`clients.${index}.default_volume`} issues={issues}>
                <Input type="number" min={0} max={100} value={client.default_volume} onChange={(event) => update((next) => { next.clients[index].default_volume = Number(event.target.value); })} className="h-11 rounded-xl" />
              </FormField>
              <FormField label={t("defaultLatency")} path={`clients.${index}.default_latency`} issues={issues}>
                <Input type="number" min={-2147483648} max={2147483647} step={1} value={client.default_latency} onChange={(event) => update((next) => { next.clients[index].default_latency = Number(event.target.value); })} className="h-11 rounded-xl" />
              </FormField>
            </div>
          </AdvancedOptions>
        </div>
      ))}
      <Button size="lg" variant="outline" onClick={addClient}>＋ {t("addClient")}</Button>
    </div>
  );
}

function SourceCard({
  title,
  description,
  enabled,
  onToggle,
  statusLabel,
  children,
}: {
  title: string;
  description: string;
  enabled: boolean;
  onToggle?: (enabled: boolean) => void;
  statusLabel?: string;
  children?: React.ReactNode;
}) {
  return (
    <div className="overflow-hidden rounded-2xl border border-border bg-background/40">
      <div className="flex min-h-16 items-center justify-between gap-4 px-4 py-3">
        <div className="min-w-0">
          <p className="font-medium">{title}</p>
          <p className="mt-0.5 text-xs leading-relaxed text-muted-foreground">{description}</p>
        </div>
        {onToggle
          ? <Switch checked={enabled} onCheckedChange={onToggle} aria-label={title} />
          : <span className="shrink-0 rounded-full bg-primary/10 px-2.5 py-1 text-xs font-medium text-primary">{statusLabel}</span>}
      </div>
      {enabled && children && <div className="space-y-4 border-t border-border px-4 py-4">{children}</div>}
    </div>
  );
}

function SourcesEditor({ config, update, issues, compact = false, t }: EditorProps & { compact?: boolean }) {
  return (
    <div className="space-y-4">
      <SourceCard
        title="AirPlay"
        description={t("airplayDescription")}
        enabled
        statusLabel={t("alwaysOn")}
      >
        <FormField label={t("airplayMode")} path="airplay.mode" issues={issues}>
          <Select value={config.airplay.mode} onChange={(event) => update((next) => { next.airplay.mode = event.target.value; })} className="h-11 rounded-xl">
            <option value="airplay2">AirPlay 2</option>
            <option value="airplay1">AirPlay 1 ({t("legacy")})</option>
          </Select>
        </FormField>
        <FormField label={t("passwordOptional")} path="airplay.password" issues={issues} description={t("airplayPasswordHint")}>
          <Input type="password" value={config.airplay.password ?? ""} onChange={(event) => update((next) => { next.airplay.password = event.target.value || null; })} className="h-11 rounded-xl" autoComplete="new-password" />
        </FormField>
        {!compact && (
          <AdvancedOptions t={t}>
            <FormField label={t("interfaceBindings")} path="airplay.bind" issues={issues}>
              <Input value={config.airplay.bind.join(", ")} onChange={(event) => update((next) => { next.airplay.bind = event.target.value.split(",").map((value) => value.trim()).filter(Boolean); })} className="h-11 rounded-xl font-mono" placeholder="192.168.1.10, ::1" />
            </FormField>
          </AdvancedOptions>
        )}
      </SourceCard>

      <SourceCard
        title="Spotify"
        description={t("spotifyDescription")}
        enabled={Boolean(config.spotify)}
        onToggle={(enabled) => update((next) => { next.spotify = enabled ? { name: next.name || "SnapDog", bitrate: 320 } : null; })}
      >
        <div className="grid gap-4 sm:grid-cols-2">
          <FormField label={t("name")} path="spotify.name" issues={issues}>
            <Input value={config.spotify?.name ?? ""} onChange={(event) => update((next) => { if (next.spotify) next.spotify.name = event.target.value; })} className="h-11 rounded-xl" />
          </FormField>
          <FormField label={t("bitrate")} path="spotify.bitrate" issues={issues}>
            <Select value={String(config.spotify?.bitrate ?? 320)} onChange={(event) => update((next) => { if (next.spotify) next.spotify.bitrate = Number(event.target.value); })} className="h-11 rounded-xl">
              <option value="96">96 kbit/s</option>
              <option value="160">160 kbit/s</option>
              <option value="320">320 kbit/s</option>
            </Select>
          </FormField>
        </div>
      </SourceCard>

      <SourceCard
        title="Subsonic"
        description={t("subsonicDescription")}
        enabled={Boolean(config.subsonic)}
        onToggle={(enabled) => update((next) => { next.subsonic = enabled ? { url: "", username: "", password: "", format: "raw", tls_skip_verify: false, cache: { path: "/data/snapdog/state/cache", max_size_mb: 2048 } } : null; })}
      >
        <FormField label={t("url")} path="subsonic.url" issues={issues}>
          <Input type="url" value={config.subsonic?.url ?? ""} onChange={(event) => update((next) => { if (next.subsonic) next.subsonic.url = event.target.value; })} className="h-11 rounded-xl" placeholder="https://music.example.com" />
        </FormField>
        <div className="grid gap-4 sm:grid-cols-2">
          <FormField label={t("username")} path="subsonic.username" issues={issues}>
            <Input value={config.subsonic?.username ?? ""} onChange={(event) => update((next) => { if (next.subsonic) next.subsonic.username = event.target.value; })} className="h-11 rounded-xl" autoComplete="username" />
          </FormField>
          <FormField label={t("password")} path="subsonic.password" issues={issues}>
            <Input type="password" value={config.subsonic?.password ?? ""} onChange={(event) => update((next) => { if (next.subsonic) next.subsonic.password = event.target.value; })} className="h-11 rounded-xl" autoComplete="current-password" />
          </FormField>
        </div>
        {!compact && (
          <>
            <FormField label={t("streamingFormat")} path="subsonic.format" issues={issues}>
              <Select value={config.subsonic?.format ?? "raw"} onChange={(event) => update((next) => { if (next.subsonic) next.subsonic.format = event.target.value; })} className="h-11 rounded-xl">
                <option value="raw">{t("originalRaw")}</option>
                <option value="flac">FLAC</option>
                <option value="mp3">MP3</option>
                <option value="opus">Opus</option>
              </Select>
            </FormField>
            <AdvancedOptions t={t}>
              <ToggleRow label={t("tlsSkipVerify")} checked={config.subsonic?.tls_skip_verify ?? false} onChange={(checked) => update((next) => { if (next.subsonic) next.subsonic.tls_skip_verify = checked; })} />
              <div className="grid gap-4 sm:grid-cols-2">
                <FormField label={t("cachePath")} path="subsonic.cache.path" issues={issues}>
                  <Input value={config.subsonic?.cache.path ?? ""} onChange={(event) => update((next) => { if (next.subsonic) next.subsonic.cache.path = event.target.value; })} className="h-11 rounded-xl font-mono" />
                </FormField>
                <FormField label={t("cacheSize")} path="subsonic.cache.max_size_mb" issues={issues}>
                  <Input type="number" min={0} max={Number.MAX_SAFE_INTEGER} step={1} value={config.subsonic?.cache.max_size_mb ?? 0} onChange={(event) => update((next) => { if (next.subsonic) next.subsonic.cache.max_size_mb = Number(event.target.value); })} className="h-11 rounded-xl" />
                </FormField>
              </div>
            </AdvancedOptions>
          </>
        )}
      </SourceCard>

      <div className="space-y-3 rounded-2xl border border-border bg-background/40 p-4">
        <div>
          <p className="font-medium">{t("radio")}</p>
          <p className="mt-0.5 text-xs text-muted-foreground">{t("radioDescription")}</p>
        </div>
        {config.radio.map((station, index) => (
          <div key={`${station.source_index ?? "new"}-${index}`} className="space-y-3 rounded-xl bg-muted/40 p-3">
            <div className="flex items-start gap-2">
              <div className="min-w-0 flex-1 space-y-3">
                <FormField label={t("stationName")} path={`radio.${index}.name`} issues={issues}>
                  <Input value={station.name} onChange={(event) => update((next) => { next.radio[index].name = event.target.value; })} className="h-11 rounded-xl" />
                </FormField>
                <FormField label={t("stationUrl")} path={`radio.${index}.url`} issues={issues}>
                  <Input type="url" value={station.url} onChange={(event) => update((next) => { next.radio[index].url = event.target.value; })} className="h-11 rounded-xl" />
                </FormField>
                <AdvancedOptions t={t}>
                  <FormField label={t("stationCover")} path={`radio.${index}.cover`} issues={issues}>
                    <Input type="url" value={station.cover ?? ""} onChange={(event) => update((next) => { next.radio[index].cover = event.target.value || null; })} className="h-11 rounded-xl" />
                  </FormField>
                </AdvancedOptions>
              </div>
              <Button size="icon-lg" variant="ghost" onClick={() => update((next) => { next.radio.splice(index, 1); })} aria-label={t("remove")}>×</Button>
            </div>
          </div>
        ))}
        <Button size="lg" variant="outline" onClick={() => update((next) => { next.radio.push({ source_index: null, name: "", url: "", cover: null }); })}>＋ {t("addStation")}</Button>
      </div>
    </div>
  );
}

function AudioEditor({ config, update, issues, t }: EditorProps) {
  const setCodec = (codec: string) => update((next) => {
    next.snapcast.codec = codec;
    if (codec === "flac" && next.audio.bit_depth > 24) next.audio.bit_depth = 24;
    if (codec.startsWith("f32")) next.audio.bit_depth = 32;
  });
  return (
    <div className="space-y-6">
      <div className="grid gap-4 sm:grid-cols-2">
        <FormField label={t("port")} path="snapcast.streaming_port" issues={issues} description={t("streamingPortDescription")}>
          <Input type="number" min={1} max={65535} value={config.snapcast.streaming_port} onChange={(event) => update((next) => { next.snapcast.streaming_port = Number(event.target.value); })} className="h-11 rounded-xl" />
        </FormField>
        <FormField label={t("codec")} path="snapcast.codec" issues={issues}>
          <Select value={config.snapcast.codec} onChange={(event) => setCodec(event.target.value)} className="h-11 rounded-xl">
            <option value="pcm">PCM</option><option value="flac">FLAC</option><option value="f32lz4">f32lz4</option><option value="f32lz4e">f32lz4e</option>
          </Select>
        </FormField>
        <FormField label={t("sampleRate")} path="audio.sample_rate" issues={issues}>
          <Select value={String(config.audio.sample_rate)} onChange={(event) => update((next) => { next.audio.sample_rate = Number(event.target.value); })} className="h-11 rounded-xl">
            {[44100, 48000, 88200, 96000, 176400, 192000].map((value) => <option key={value} value={value}>{value / 1000} kHz</option>)}
          </Select>
        </FormField>
        <FormField label={t("bitDepth")} path="audio.bit_depth" issues={issues}>
          <Select value={String(config.audio.bit_depth)} disabled={config.snapcast.codec.startsWith("f32")} onChange={(event) => update((next) => { next.audio.bit_depth = Number(event.target.value); })} className="h-11 rounded-xl">
            <option value="16">16 bit</option><option value="24">24 bit</option>{config.snapcast.codec !== "flac" && <option value="32">32 bit</option>}
          </Select>
        </FormField>
      </div>
      {config.snapcast.codec === "f32lz4e" && (
        <FormField label={t("psk")} path="snapcast.encryption_psk" issues={issues}>
          <Input type="password" value={config.snapcast.encryption_psk ?? ""} onChange={(event) => update((next) => { next.snapcast.encryption_psk = event.target.value || null; })} className="h-11 rounded-xl" autoComplete="new-password" />
        </FormField>
      )}
      <div className="grid gap-4 sm:grid-cols-2">
        <FormField label={t("groupVolume")} path="snapcast.group_volume_mode" issues={issues}>
          <Select value={config.snapcast.group_volume_mode} onChange={(event) => update((next) => { next.snapcast.group_volume_mode = event.target.value; })} className="h-11 rounded-xl">
            <option value="relative">{t("relative")}</option><option value="absolute">{t("absolute")}</option><option value="compressed">{t("compressed")}</option>
          </Select>
        </FormField>
        <FormField label={t("unknownClients")} path="snapcast.unknown_clients" issues={issues}>
          <Select value={config.snapcast.unknown_clients} onChange={(event) => update((next) => { next.snapcast.unknown_clients = event.target.value; })} className="h-11 rounded-xl">
            <option value="accept">{t("accept")}</option><option value="ignore">{t("ignore")}</option><option value="reject">{t("reject")}</option>
          </Select>
        </FormField>
        <FormField label={t("defaultZone")} path="snapcast.default_zone" issues={issues}>
          <Select value={config.snapcast.default_zone ?? ""} onChange={(event) => update((next) => { next.snapcast.default_zone = event.target.value || null; })} className="h-11 rounded-xl">
            <option value="">{t("automatic")}</option>{config.zones.map((zone) => <option key={zone.name} value={zone.name}>{zone.name}</option>)}
          </Select>
        </FormField>
        <FormField label={t("logLevel")} path="system.log_level" issues={issues}>
          <Select value={config.system.log_level} onChange={(event) => update((next) => { next.system.log_level = event.target.value; })} className="h-11 rounded-xl">
            {['error', 'warn', 'info', 'debug', 'trace'].map((level) => <option key={level} value={level}>{level}</option>)}
          </Select>
        </FormField>
      </div>
      <div className="grid gap-4 sm:grid-cols-2">
        <FormField label={t("zoneSwitchFade")} path="audio.zone_switch_fade_ms" issues={issues}>
          <Input type="number" min={0} max={1000} step={50} value={config.audio.zone_switch_fade_ms} onChange={(event) => update((next) => { next.audio.zone_switch_fade_ms = Number(event.target.value); })} className="h-11 rounded-xl" />
        </FormField>
        <FormField label={t("sourceSwitchFade")} path="audio.source_switch_fade_ms" issues={issues}>
          <Input type="number" min={0} max={1000} step={50} value={config.audio.source_switch_fade_ms} onChange={(event) => update((next) => { next.audio.source_switch_fade_ms = Number(event.target.value); })} className="h-11 rounded-xl" />
        </FormField>
      </div>
      <AdvancedOptions t={t}>
        <div className="grid gap-4 sm:grid-cols-2">
          <FormField label={t("channelCount")} path="audio.channels" issues={issues}>
            <Input type="number" min={1} max={8} value={config.audio.channels} onChange={(event) => update((next) => { next.audio.channels = Number(event.target.value); })} className="h-11 rounded-xl" />
          </FormField>
          <FormField label={t("sourceConflict")} path="audio.source_conflict" issues={issues}>
            <Select value={config.audio.source_conflict} onChange={(event) => update((next) => { next.audio.source_conflict = event.target.value; })} className="h-11 rounded-xl">
              <option value="last_wins">{t("lastWins")}</option><option value="receiver_wins">{t("receiverWins")}</option>
            </Select>
          </FormField>
          <FormField label={t("snapcastAddress")} path="snapcast.address" issues={issues}>
            <Input value={config.snapcast.address} onChange={(event) => update((next) => { next.snapcast.address = event.target.value; })} className="h-11 rounded-xl font-mono" />
          </FormField>
          <FormField label={t("jsonRpcPort")} path="snapcast.jsonrpc_tcp_port" issues={issues}>
            <Input type="number" min={1} max={65535} value={config.snapcast.jsonrpc_tcp_port} onChange={(event) => update((next) => { next.snapcast.jsonrpc_tcp_port = Number(event.target.value); })} className="h-11 rounded-xl" />
          </FormField>
          <FormField label={t("logFile")} path="system.log_file" issues={issues}>
            <Input value={config.system.log_file ?? ""} onChange={(event) => update((next) => { next.system.log_file = event.target.value || null; })} className="h-11 rounded-xl font-mono" />
          </FormField>
          <FormField label={t("stateDirectory")} path="system.state_dir" issues={issues}>
            <Input value={config.system.state_dir} onChange={(event) => update((next) => { next.system.state_dir = event.target.value; })} className="h-11 rounded-xl font-mono" />
          </FormField>
        </div>
        <ToggleRow label={t("managedSnapcast")} checked={config.snapcast.managed} onChange={(checked) => update((next) => { next.snapcast.managed = checked; })} />
        <ToggleRow label={t("verboseLogging")} checked={config.snapcast.verbose} onChange={(checked) => update((next) => { next.snapcast.verbose = checked; })} />
      </AdvancedOptions>
    </div>
  );
}

function IntegrationsEditor({ config, update, issues, t }: EditorProps) {
  return (
    <div className="space-y-5">
      <div className="space-y-3 rounded-2xl border border-border bg-background/40 p-4">
        <div>
          <p className="font-medium">{t("apiKeys")}</p>
          <p className="mt-0.5 text-xs text-muted-foreground">{t("apiKeysDescription")}</p>
        </div>
        {config.http.api_keys.map((key, index) => (
          <div key={index} className="flex items-center gap-2">
            <Input type="password" value={key} onChange={(event) => update((next) => { next.http.api_keys[index] = event.target.value; })} className="h-11 rounded-xl font-mono" aria-label={`${t("apiKeys")} ${index + 1}`} autoComplete="off" />
            <Button size="icon-lg" variant="ghost" onClick={() => update((next) => { next.http.api_keys.splice(index, 1); })} aria-label={t("removeKey")}>×</Button>
          </div>
        ))}
        <Button size="lg" variant="outline" onClick={() => update((next) => { next.http.api_keys.push(""); })}>＋ {t("addKey")}</Button>
      </div>

      <SourceCard
        title="MQTT"
        description={t("mqttDescription")}
        enabled={Boolean(config.mqtt)}
        onToggle={(enabled) => update((next) => { next.mqtt = enabled ? { broker: "", client_id: "snapdog", username: null, password: null, base_topic: "snapdog/" } : null; })}
      >
        <FormField label={t("broker")} path="mqtt.broker" issues={issues}>
          <Input value={config.mqtt?.broker ?? ""} onChange={(event) => update((next) => { if (next.mqtt) next.mqtt.broker = event.target.value; })} className="h-11 rounded-xl" placeholder="mqtt://192.168.1.10:1883" />
        </FormField>
        <div className="grid gap-4 sm:grid-cols-2">
          <FormField label={t("username")} path="mqtt.username" issues={issues}>
            <Input value={config.mqtt?.username ?? ""} onChange={(event) => update((next) => { if (next.mqtt) next.mqtt.username = event.target.value || null; })} className="h-11 rounded-xl" />
          </FormField>
          <FormField label={t("password")} path="mqtt.password" issues={issues}>
            <Input type="password" value={config.mqtt?.password ?? ""} onChange={(event) => update((next) => { if (next.mqtt) next.mqtt.password = event.target.value || null; })} className="h-11 rounded-xl" autoComplete="current-password" />
          </FormField>
        </div>
        <FormField label={t("baseTopic")} path="mqtt.base_topic" issues={issues}>
          <Input value={config.mqtt?.base_topic ?? "snapdog/"} onChange={(event) => update((next) => { if (next.mqtt) next.mqtt.base_topic = event.target.value; })} className="h-11 rounded-xl" />
        </FormField>
        <AdvancedOptions t={t}>
          <FormField label={t("mqttClientId")} path="mqtt.client_id" issues={issues}>
            <Input value={config.mqtt?.client_id ?? "snapdog"} onChange={(event) => update((next) => { if (next.mqtt) next.mqtt.client_id = event.target.value; })} className="h-11 rounded-xl font-mono" />
          </FormField>
        </AdvancedOptions>
      </SourceCard>

      <SourceCard
        title="KNX"
        description={t("knxDescription")}
        enabled={Boolean(config.knx)}
        onToggle={(enabled) => update((next) => { next.knx = enabled ? { role: "client", url: null, individual_address: null, persist_ets_config: null, restart_after_ets: null, start_prog_mode: false, server_online: null, all_stop: null, all_mute: null, all_mute_status: null, system_fault: null, knx_time: null, heartbeat_minutes: 5, sync_system_clock: false } : null; })}
      >
        <div className="grid gap-4 sm:grid-cols-2">
          <FormField label={t("knxMode")} path="knx.role" issues={issues}>
            <Select value={config.knx?.role ?? "client"} onChange={(event) => update((next) => { if (next.knx) { next.knx.role = event.target.value as "client" | "device"; if (next.knx.role === "device") next.knx.url = null; } })} className="h-11 rounded-xl">
              <option value="client">{t("knxClient")}</option><option value="device">{t("knxDevice")}</option>
            </Select>
          </FormField>
          {config.knx?.role === "client" && (
            <FormField label={t("gatewayUrl")} path="knx.url" issues={issues}>
              <Input value={config.knx.url ?? ""} onChange={(event) => update((next) => { if (next.knx) next.knx.url = event.target.value || null; })} className="h-11 rounded-xl" placeholder="knxip://192.168.1.20:3671" />
            </FormField>
          )}
        </div>
        <AdvancedOptions t={t}>
          <div className="grid gap-4 sm:grid-cols-2">
            <FormField label={t("knxIndividualAddress")} path="knx.individual_address" issues={issues}>
              <Input value={config.knx?.individual_address ?? ""} onChange={(event) => update((next) => { if (next.knx) next.knx.individual_address = event.target.value || null; })} className="h-11 rounded-xl font-mono" placeholder="1.1.1" />
            </FormField>
            <FormField label={t("heartbeatMinutes")} path="knx.heartbeat_minutes" issues={issues}>
              <Input type="number" min={0} max={65535} step={1} value={config.knx?.heartbeat_minutes ?? 5} onChange={(event) => update((next) => { if (next.knx) next.knx.heartbeat_minutes = Number(event.target.value); })} className="h-11 rounded-xl" />
            </FormField>
            <FormField label={t("persistEtsConfig")} path="knx.persist_ets_config" issues={issues}>
              <Select value={config.knx?.persist_ets_config == null ? "" : String(config.knx.persist_ets_config)} onChange={(event) => update((next) => { if (next.knx) next.knx.persist_ets_config = event.target.value === "" ? null : event.target.value === "true"; })} className="h-11 rounded-xl">
                <option value="">{t("automatic")}</option><option value="true">{t("enabled")}</option><option value="false">{t("disabledShort")}</option>
              </Select>
            </FormField>
            <FormField label={t("restartAfterEts")} path="knx.restart_after_ets" issues={issues}>
              <Select value={config.knx?.restart_after_ets == null ? "" : String(config.knx.restart_after_ets)} onChange={(event) => update((next) => { if (next.knx) next.knx.restart_after_ets = event.target.value === "" ? null : event.target.value === "true"; })} className="h-11 rounded-xl">
                <option value="">{t("automatic")}</option><option value="true">{t("enabled")}</option><option value="false">{t("disabledShort")}</option>
              </Select>
            </FormField>
          </div>
          <ToggleRow label={t("programmingMode")} checked={config.knx?.start_prog_mode ?? false} onChange={(checked) => update((next) => { if (next.knx) next.knx.start_prog_mode = checked; })} />
          <ToggleRow label={t("syncSystemClock")} checked={config.knx?.sync_system_clock ?? false} onChange={(checked) => update((next) => { if (next.knx) next.knx.sync_system_clock = checked; })} />
          <div>
            <p className="mb-3 text-xs font-semibold uppercase tracking-[0.12em] text-muted-foreground">{t("systemGroupObjects")}</p>
            <div className="grid gap-3 sm:grid-cols-2">
              {SYSTEM_KNX_KEYS.map((key) => (
                <FormField key={key} label={key} path={`knx.${key}`} issues={issues}>
                  <Input value={config.knx?.[key] ?? ""} onChange={(event) => update((next) => { if (next.knx) next.knx[key] = event.target.value || null; })} className="h-10 rounded-xl font-mono text-xs" placeholder="1/2/3 · 1/1234" />
                </FormField>
              ))}
            </div>
          </div>
        </AdvancedOptions>
        <KnxObjectEditor config={config} update={update} issues={issues} t={t} />
      </SourceCard>
    </div>
  );
}

function KnxObjectEditor({ config, update, issues, t }: EditorProps) {
  const setZoneValue = (zoneIndex: number, key: typeof ZONE_KNX_KEYS[number], value: string) => update((next) => {
    const values = { ...(next.zones[zoneIndex].knx ?? {}) };
    values[key] = value.trim() || null;
    next.zones[zoneIndex].knx = Object.values(values).some(Boolean) ? values : null;
  });
  const setClientValue = (clientIndex: number, key: typeof CLIENT_KNX_KEYS[number], value: string) => update((next) => {
    const values = { ...(next.clients[clientIndex].knx ?? {}) };
    values[key] = value.trim() || null;
    next.clients[clientIndex].knx = Object.values(values).some(Boolean) ? values : null;
  });
  return (
    <div className="space-y-3">
      <p className="text-xs font-semibold uppercase tracking-[0.12em] text-muted-foreground">{t("knxGos")}</p>
      {config.zones.map((zone, zoneIndex) => (
        <details key={`${zone.name}-${zoneIndex}`} className="rounded-xl border border-border bg-card">
          <summary className="cursor-pointer px-4 py-3 text-sm font-medium">{zone.icon} {zone.name} · {t("zone")}</summary>
          <div className="grid gap-3 border-t border-border p-3 sm:grid-cols-2">
            {ZONE_KNX_KEYS.map((key) => <KnxField key={key} name={key} path={`zones.${zoneIndex}.knx.${key}`} issues={issues} value={zone.knx?.[key] ?? ""} onChange={(value) => setZoneValue(zoneIndex, key, value)} />)}
          </div>
        </details>
      ))}
      {config.clients.map((client, clientIndex) => (
        <details key={`${client.mac}-${clientIndex}`} className="rounded-xl border border-border bg-card">
          <summary className="cursor-pointer px-4 py-3 text-sm font-medium">{client.icon} {client.name || t("newClient")} · {t("clientName")}</summary>
          <div className="grid gap-3 border-t border-border p-3 sm:grid-cols-2">
            {CLIENT_KNX_KEYS.map((key) => <KnxField key={key} name={key} path={`clients.${clientIndex}.knx.${key}`} issues={issues} value={client.knx?.[key] ?? ""} onChange={(value) => setClientValue(clientIndex, key, value)} />)}
          </div>
        </details>
      ))}
    </div>
  );
}

function KnxField({ name, path, issues, value, onChange }: { name: string; path: string; issues: ServerConfigIssue[]; value: string; onChange: (value: string) => void }) {
  return (
    <FormField label={name} path={path} issues={issues}>
      <Input value={value} onChange={(event) => onChange(event.target.value)} placeholder="1/2/3 · 1/1234" className="h-10 rounded-xl font-mono text-xs" />
    </FormField>
  );
}

function AdvancedEditor({ config, update, issues, t }: EditorProps) {
  const tomlIssue = config.raw_toml_changed
    ? issues[0]
    : issues.find((issue) => canonicalPath(issue.field_path ?? issue.field).startsWith("raw_toml") || issue.line);
  const issueId = useId();
  return (
    <div className="space-y-4">
      <InlineNotice tone="warning">
        <p className="font-medium">{t("advancedWarningTitle")}</p>
        <p className="mt-1 text-xs leading-relaxed opacity-90">{t("advancedDescription")}</p>
      </InlineNotice>
      <div className="space-y-1.5">
        <div className="flex flex-wrap items-center justify-between gap-2">
          <label htmlFor="server-raw-toml" className="text-sm font-medium">/etc/snapdog/snapdog.toml</label>
          <span className="text-xs text-muted-foreground">{t("advancedNotPersisted")}</span>
        </div>
        <textarea
          id="server-raw-toml"
          data-server-field="raw_toml"
          value={config.raw_toml}
          onChange={(event) => update((next) => { next.raw_toml = event.target.value; next.raw_toml_changed = true; })}
          spellCheck={false}
          aria-invalid={Boolean(tomlIssue)}
          aria-describedby={tomlIssue ? issueId : undefined}
          className="min-h-[28rem] w-full resize-y rounded-2xl border border-border bg-background px-4 py-3 font-mono text-xs leading-relaxed outline-none transition focus:border-primary focus:ring-2 focus:ring-primary/20"
        />
        {tomlIssue && (
          <p id={issueId} className="text-xs text-destructive" role="alert">
            {tomlIssue.message}{tomlIssue.line ? ` · ${t("lineColumn", { line: tomlIssue.line, column: tomlIssue.column ?? 0 })}` : ""}
          </p>
        )}
      </div>
      <p className="text-xs leading-relaxed text-muted-foreground">{t("advancedPreservation")}</p>
    </div>
  );
}

function phaseIndex(phase: ApplyPhase, operationPhase?: string): number {
  const current = operationPhase ?? phase;
  if (["validating"].includes(current)) return 0;
  if (["staging", "activating", "applying"].includes(current)) return 1;
  if (["starting", "restarting", "stopping"].includes(current)) return 2;
  if (["verifying", "checking", "health_check"].includes(current)) return 3;
  return -1;
}

function StructuredChangeReview({
  base,
  draft,
  t,
}: {
  base: ServerConfig;
  draft: ServerConfig;
  t: (key: string, values?: Record<string, string | number>) => string;
}) {
  const changes = structuredConfigChanges(base, draft);
  const sectionLabels: Record<SettingsSection, string> = {
    overview: t("configuration"),
    general: t("settingsGeneral"),
    zones: t("zones"),
    clients: t("clients"),
    sources: t("subtabSources"),
    audio: t("audioAndStreaming"),
    integrations: t("subtabIntegrations"),
    advanced: t("subtabAdvanced"),
  };
  const groups = [...new Set(changes.map((change) => sectionForPath(change.path)))];
  const visibleLimit = 16;
  let rendered = 0;

  return (
    <div className="space-y-3">
      <p className="text-sm font-medium">{t("changesReviewTitle")}</p>
      {groups.map((sectionName) => {
        const sectionChanges = changes.filter((change) => sectionForPath(change.path) === sectionName);
        const visible = sectionChanges.slice(0, Math.max(0, visibleLimit - rendered));
        rendered += visible.length;
        return (
          <div key={sectionName} className="overflow-hidden rounded-2xl border border-border bg-background/45">
            <div className="flex items-center justify-between gap-3 border-b border-border px-4 py-3">
              <p className="text-sm font-semibold">{sectionLabels[sectionName]}</p>
              <span className="rounded-full bg-muted px-2 py-0.5 text-xs text-muted-foreground">{t("changeCount", { count: sectionChanges.length })}</span>
            </div>
            <ul className="divide-y divide-border">
              {visible.map((change) => (
                <li key={change.path} className="space-y-1 px-4 py-3">
                  <code className="block break-all text-[11px] text-muted-foreground">{change.path.replace(/\.(\d+)/g, "[$1]")}</code>
                  {change.sensitive ? (
                    <p className="text-xs font-medium">{t("sensitiveValueChanged")}</p>
                  ) : (
                    <p className="flex flex-wrap items-center gap-1.5 text-xs">
                      <span className="sr-only">{t("fromValue")}</span><span className="max-w-full break-all rounded-md bg-muted px-1.5 py-0.5">{displayConfigValue(change.before)}</span>
                      <span aria-hidden="true">→</span>
                      <span className="sr-only">{t("toValue")}</span><span className="max-w-full break-all rounded-md bg-primary/10 px-1.5 py-0.5">{displayConfigValue(change.after)}</span>
                    </p>
                  )}
                </li>
              ))}
            </ul>
          </div>
        );
      })}
      {changes.length > visibleLimit && <p className="text-xs text-muted-foreground">{t("additionalChanges", { count: changes.length - visibleLimit })}</p>}
    </div>
  );
}

function RawTomlChangeReview({
  base,
  draft,
  t,
}: {
  base: ServerConfig;
  draft: ServerConfig;
  t: (key: string, values?: Record<string, string | number>) => string;
}) {
  const delta = rawTomlLineDelta(base.raw_toml, draft.raw_toml);
  return (
    <div className="rounded-2xl border border-amber-500/25 bg-amber-500/10 p-4 text-amber-950 dark:text-amber-100">
      <p className="text-sm font-semibold">{t("rawTomlReviewTitle")}</p>
      <p className="mt-1 text-xs leading-relaxed opacity-85">{t("rawTomlReviewDescription")}</p>
      <dl className="mt-3 grid grid-cols-2 gap-3">
        <div className="rounded-xl bg-background/45 px-3 py-2"><dt className="text-[11px] opacity-75">{t("linesAdded")}</dt><dd className="mt-0.5 font-mono text-sm font-semibold">+{delta.added}</dd></div>
        <div className="rounded-xl bg-background/45 px-3 py-2"><dt className="text-[11px] opacity-75">{t("linesRemoved")}</dt><dd className="mt-0.5 font-mono text-sm font-semibold">−{delta.removed}</dd></div>
      </dl>
    </div>
  );
}

function ReviewSheet({
  open,
  base,
  config,
  state,
  guidedSetup,
  phase,
  onCancel,
  onApply,
  t,
}: {
  open: boolean;
  base: ServerConfig | null;
  config: ServerConfig | null;
  state: ServerState | null;
  guidedSetup: boolean;
  phase: ApplyPhase;
  onCancel: () => void;
  onApply: () => void;
  t: (key: string, values?: Record<string, string | number>) => string;
}) {
  const trapRef = useFocusTrap<HTMLDivElement>(open);
  const titleId = useId();
  const descriptionId = useId();
  const applying = phase !== "idle" || Boolean(state?.operation);
  const currentPhase = phaseIndex(phase, state?.operation?.phase);
  const exceptionalPhase = state?.operation?.phase === "rolling_back"
    ? t("phaseRollingBack")
    : state?.operation?.phase === "recovering"
      ? t("phaseRecovering")
      : null;
  const isFirstSetup = state?.setup_state === "needs_setup";
  const steps = [
    t("phaseValidating"),
    t("phaseApplying"),
    isFirstSetup ? t("phaseStarting") : state?.desired_state === "stopped" ? t("phaseSaving") : t("phaseRestarting"),
    t("phaseChecking"),
  ];
  useEffect(() => {
    if (open && applying) trapRef.current?.focus();
  }, [applying, open, trapRef]);
  if (!open || !config) return null;
  return (
    <div
      className="fixed inset-0 z-50 flex items-end justify-center sm:items-center"
      role="dialog"
      aria-modal="true"
      aria-labelledby={titleId}
      aria-describedby={descriptionId}
      onKeyDown={(event) => { if (event.key === "Escape" && !applying) onCancel(); }}
    >
      <div role="presentation" className="absolute inset-0 bg-background/80 backdrop-blur-md" onClick={applying ? undefined : onCancel} />
      <div ref={trapRef} tabIndex={-1} className="relative z-10 max-h-[90vh] w-full overflow-y-auto rounded-t-3xl border border-border bg-card p-5 shadow-2xl outline-none sm:mx-4 sm:max-w-lg sm:rounded-3xl sm:p-6">
        <p className="text-xs font-semibold uppercase tracking-[0.16em] text-primary">{t("reviewEyebrow")}</p>
        <h2 id={titleId} className="mt-2 text-2xl font-semibold tracking-tight">{applying ? t("applyingTitle") : t("reviewTitle")}</h2>
        <p id={descriptionId} className="mt-2 text-sm leading-relaxed text-muted-foreground">
          {applying
            ? t("applyingDescription")
            : isFirstSetup
              ? t("wizardReviewDescription")
              : state?.desired_state === "stopped"
                ? t("reviewOffDescription")
                : t("reviewDescription")}
        </p>

        {applying && exceptionalPhase ? (
          <div className="mt-6 flex items-center gap-3 rounded-2xl border border-amber-500/25 bg-amber-500/10 px-4 py-4 text-amber-900 dark:text-amber-200" aria-live="assertive">
            <span className="size-4 animate-spin rounded-full border-2 border-current border-r-transparent motion-reduce:animate-none" aria-hidden="true" />
            <span className="text-sm font-medium">{exceptionalPhase}</span>
          </div>
        ) : applying ? (
          <ol className="mt-6 space-y-3" aria-live="polite">
            {steps.map((label, index) => {
              const done = currentPhase > index;
              const active = currentPhase === index;
              return (
                <li key={label} className={`flex items-center gap-3 rounded-2xl px-4 py-3 ${active ? "bg-primary/10" : "bg-muted/35"}`}>
                  <span className={`flex size-7 items-center justify-center rounded-full text-xs ${done ? "bg-green-500 text-white" : active ? "bg-primary text-primary-foreground" : "bg-muted text-muted-foreground"}`}>
                    {done ? "✓" : active ? <span className="size-3 animate-spin rounded-full border border-current border-r-transparent motion-reduce:animate-none" /> : index + 1}
                  </span>
                  <span className="text-sm font-medium">{label}</span>
                </li>
              );
            })}
          </ol>
        ) : (
          <div className="mt-6">
            {config.raw_toml_changed && base
              ? <RawTomlChangeReview base={base} draft={config} t={t} />
              : guidedSetup || !base
                ? <SetupSummary config={config} t={t} />
                : <StructuredChangeReview base={base} draft={config} t={t} />}
          </div>
        )}

        {!applying && (
          <div className="mt-6 flex flex-col-reverse gap-2 sm:flex-row">
            <Button size="lg" variant="outline" className="w-full" onClick={onCancel}>{t("cancel")}</Button>
            <Button size="lg" className="w-full" onClick={onApply}>{state?.setup_state === "needs_setup" ? t("startServer") : t("applyChanges")}</Button>
          </div>
        )}
      </div>
    </div>
  );
}

function DiagnosticsSheet({
  open,
  loading,
  diagnostics,
  error,
  onRetry,
  onClose,
  t,
}: {
  open: boolean;
  loading: boolean;
  diagnostics: ServerDiagnostics | null;
  error: string | null;
  onRetry: () => void;
  onClose: () => void;
  t: (key: string) => string;
}) {
  const trapRef = useFocusTrap<HTMLDivElement>(open);
  const titleId = useId();
  const copy = async () => {
    if (!diagnostics) return;
    const text = JSON.stringify(diagnostics, null, 2);
    try { await copyText(text); } catch { /* Copy is unavailable in this browser. */ }
  };
  if (!open) return null;
  return (
    <div
      className="fixed inset-0 z-50 flex items-end justify-center sm:items-center"
      role="dialog"
      aria-modal="true"
      aria-labelledby={titleId}
      onKeyDown={(event) => { if (event.key === "Escape") onClose(); }}
    >
      <div role="presentation" className="absolute inset-0 bg-background/80 backdrop-blur-md" onClick={onClose} />
      <div ref={trapRef} tabIndex={-1} className="relative z-10 max-h-[90vh] w-full overflow-y-auto rounded-t-3xl border border-border bg-card p-5 shadow-2xl outline-none sm:mx-4 sm:max-w-xl sm:rounded-3xl sm:p-6">
        <div className="flex items-start justify-between gap-4">
          <div><p className="text-xs font-semibold uppercase tracking-[0.16em] text-muted-foreground">{t("diagnosticsEyebrow")}</p><h2 id={titleId} className="mt-1 text-xl font-semibold">{t("diagnosticsTitle")}</h2></div>
          <Button size="icon-lg" variant="ghost" onClick={onClose} aria-label={t("close")}>×</Button>
        </div>
        {loading ? <Skeleton className="mt-5 h-64 w-full rounded-2xl" /> : diagnostics ? (
          <div className="mt-5 space-y-4">
            {diagnostics.state.issue && (
              <InlineNotice tone={diagnostics.state.issue.rollback_succeeded ? "warning" : "error"}>
                <p className="font-medium">{diagnostics.state.issue.summary}</p>
                {diagnostics.state.issue.detail && <p className="mt-1 text-xs">{diagnostics.state.issue.detail}</p>}
              </InlineNotice>
            )}
            <dl className="grid grid-cols-2 gap-x-4 gap-y-2 rounded-2xl bg-muted/40 p-4 text-sm">
              <dt className="text-muted-foreground">{t("serviceState")}</dt><dd>{diagnostics.systemd.active_state || "—"} / {diagnostics.systemd.sub_state || "—"}</dd>
              <dt className="text-muted-foreground">{t("serviceResult")}</dt><dd>{diagnostics.systemd.result || "—"}</dd>
              <dt className="text-muted-foreground">{t("restartCount")}</dt><dd>{diagnostics.systemd.restart_count ?? 0}</dd>
              <dt className="text-muted-foreground">{t("exitCode")}</dt><dd>{diagnostics.systemd.exec_main_status ?? "—"}</dd>
            </dl>
            <div className="space-y-2">
              <p className="text-sm font-medium">{t("recentLogs")}</p>
              <pre className="max-h-64 overflow-auto rounded-2xl bg-muted p-4 font-mono text-[11px] leading-relaxed whitespace-pre-wrap">{diagnostics.journal.join("\n") || t("noLogs")}</pre>
            </div>
            <div className="flex flex-wrap gap-2"><Button size="lg" variant="outline" onClick={() => void copy()}>{t("copyDiagnostics")}</Button><Button size="lg" onClick={onClose}>{t("done")}</Button></div>
          </div>
        ) : (
          <div className="mt-5 space-y-4">
            <InlineNotice tone="error">{error || t("diagnosticsFailed")}</InlineNotice>
            <div className="flex flex-wrap gap-2">
              <Button size="lg" onClick={onRetry}>{t("reload")}</Button>
              <Button size="lg" variant="outline" onClick={onClose}>{t("close")}</Button>
            </div>
          </div>
        )}
      </div>
    </div>
  );
}
