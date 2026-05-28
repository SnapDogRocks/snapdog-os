"use client";

import { useState, useEffect, useCallback, useId, useRef } from "react";
import { useTranslations } from "next-intl";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Skeleton } from "@/components/ui/skeleton";
import { Select } from "@/components/ui/select";
import { Switch } from "@/components/ui/switch";
import { AboutButton } from "@/components/AboutButton";
import {
  api,
  type SystemInfo,
  type WifiNetwork,
  type AudioConfig,
  type ClientConfig,
  type SshConfig,
  type ServerConfig,
  type ServerStatus,
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
    bit_depth: 16,
    channels: 2,
    source_conflict: "last_wins",
    zone_switch_fade_ms: 300,
    source_switch_fade_ms: 300,
  },
  snapcast: {
    streaming_port: 1704,
    codec: "flac",
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
  zones: [{ name: "Default", icon: "🔊" }],
  clients: [],
  radio: [],
  system: { log_level: "info" },
};

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
      <dl className="grid grid-cols-[auto_1fr] gap-x-4 gap-y-3 text-sm">
        <dt className="text-muted-foreground">{t("hostname")}</dt>
        <dd className="font-medium">{info.hostname || "—"}</dd>
        <dt className="text-muted-foreground">{t("version")}</dt>
        <dd className="font-mono text-xs">
          <span>{info.version || "—"}</span>
          <div className="mt-1 text-[10px] text-muted-foreground">
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
        <dt className="text-muted-foreground">{t("piVersion")}</dt>
        <dd>Raspberry Pi {info.pi_version}</dd>
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

function NetworkTab() {
  const t = useTranslations("network");
  const [networks, setNetworks] = useState<WifiNetwork[]>([]);
  const [scanning, setScanning] = useState(true);
  const [ssid, setSsid] = useState("");
  const [password, setPassword] = useState("");
  const [wifiMode, setWifiMode] = useState<"dhcp" | "static">("dhcp");
  const [wifiIp, setWifiIp] = useState("");
  const [wifiSubnet, setWifiSubnet] = useState("255.255.255.0");
  const [wifiGateway, setWifiGateway] = useState("");
  const [wifiDns, setWifiDns] = useState("");
  const [wifiStatus, setWifiStatus] = useState<import("@/lib/api").WifiStatus | null>(null);
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

  const scan = useCallback(() => {
    setScanning(true);
    api.scanWifi().then((r) => setNetworks(r.networks)).catch(() => {}).finally(() => setScanning(false));
  }, []);

  useEffect(() => {
    api.scanWifi().then((r) => setNetworks(r.networks)).catch(() => {}).finally(() => setScanning(false));
    api.getWifi().then((w) => {
      setWifiStatus(w);
      if (w.mode) setWifiMode(w.mode);
    }).catch(() => {});
    api.getEthernet().then((e) => {
      setEthStatus(e);
      if (e.mode) setEthMode(e.mode as "dhcp" | "static");
      if (e.ip) setEthIp(e.ip);
      if (e.subnet) setEthSubnet(e.subnet);
      if (e.gateway) setEthGateway(e.gateway);
      if (e.dns) setEthDns(e.dns);
    }).catch(() => {});
  }, []);

  return (
    <div className="space-y-5">
      <Card title={t("wifi")} id={wifiCardId}>
        <div className="space-y-3">
          {wifiStatus?.connected && (
            <div className="flex items-center gap-2 text-sm">
              <StatusDot connected label={t("connectedTo")} />
              <span className="font-medium">{wifiStatus.ssid}</span>
              <span className="text-xs text-muted-foreground">({wifiStatus.signal} dBm)</span>
            </div>
          )}
          {wifiStatus?.connected && (
            <NetworkDetails ip={wifiStatus.ip} subnet={wifiStatus.subnet} gateway={wifiStatus.gateway} dns={wifiStatus.dns} />
          )}
          <Button variant="outline" size="sm" onClick={scan} disabled={scanning} aria-busy={scanning}>
            {scanning ? t("scanning") : t("scan")}
          </Button>
          {networks.length > 0 && (
            <ul className="max-h-40 space-y-1 overflow-y-auto text-sm" aria-label={t("availableNetworks")}>
              {networks.map((n) => (
                <li key={n.ssid}>
                  <button
                    type="button"
                    className="flex w-full items-center justify-between rounded-lg px-2 py-1.5 text-left hover:bg-muted focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
                    onClick={() => setSsid(n.ssid)}
                    aria-label={`${t("selectNetwork")}: ${n.ssid} (${n.signal} dBm)`}
                  >
                    <span>{n.ssid}</span>
                    <span className="text-xs text-muted-foreground" aria-hidden="true">{n.signal} dBm</span>
                  </button>
                </li>
              ))}
            </ul>
          )}
          <Field label={t("ssid")} htmlFor={ssidId}>
            <Input id={ssidId} value={ssid} onChange={(e) => setSsid(e.target.value)} autoComplete="off" />
          </Field>
          <Field label={t("password")} htmlFor={passwordId}>
            <Input id={passwordId} type="password" value={password} onChange={(e) => setPassword(e.target.value)} autoComplete="current-password" />
          </Field>
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
          <Button size="sm" onClick={() => api.setWifi({ ssid, password, mode: wifiMode, ...(wifiMode === "static" ? { ip: wifiIp, subnet: wifiSubnet, gateway: wifiGateway, dns: wifiDns } : {}) })}>
            {t("connect")}
          </Button>
          {wifiStatus?.connected && (
            <Button variant="outline" size="sm" onClick={() => api.disconnectWifi()}>
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

function SoftApCard() {
  const id = useId();
  const [config, setConfig] = useState({ enabled: true, password: "snapdog123" });
  const [status, setStatus] = useState<"idle" | "saved">("idle");

  useEffect(() => {
    api.getSoftap().then(setConfig).catch(() => {});
  }, []);

  const save = (updated: typeof config) => {
    setConfig(updated);
    api.setSoftap(updated).then(() => { setStatus("saved"); setTimeout(() => setStatus("idle"), 2000); });
  };

  return (
    <Card title="Setup Access Point" id={id}>
      <div className="space-y-3">
        <p className="text-xs text-muted-foreground">
          Creates a temporary WiFi network on each boot when no WiFi is configured and no Ethernet is connected. Stops automatically once a network connection is established.
        </p>
        <div className="flex items-center justify-between">
          <span className="text-sm">Enable Setup AP</span>
          <Switch checked={config.enabled} onCheckedChange={(enabled) => save({ ...config, enabled })} />
        </div>
        {config.enabled && (
          <div>
            <label htmlFor={`${id}-pw`} className="text-sm font-medium">AP Password</label>
            <Input id={`${id}-pw`} value={config.password} onChange={(e) => setConfig({ ...config, password: e.target.value })}
              onBlur={() => { if (config.password.length >= 8) save(config); }} minLength={8} />
            {config.password.length < 8 && <p className="text-xs text-destructive">Minimum 8 characters (WPA2)</p>}
          </div>
        )}
        {status === "saved" && <p className="text-xs text-green-600">Saved</p>}
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

  return (
    <Card title={t("title")} id={cardId}>
      <div className="space-y-3">
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
        <Field label={t("detectedCard")}>
          <p className="font-mono text-xs text-foreground">{config.detected_card || "—"}</p>
        </Field>
      </div>
    </Card>
  );
}

// ── Client Tab ────────────────────────────────────────────────

function ClientTab() {
  const t = useTranslations("client");
  const [config, setConfig] = useState<ClientConfig>({ server_url: "", host_id: "", soundcard: "default", mixer: "", latency: 0 });
  const [soundcards, setSoundcards] = useState<string[]>([]);
  const [servers, setServers] = useState<{ name: string; host: string; port: number }[]>([]);
  const [scanning, setScanning] = useState(true);
  const [manualHost, setManualHost] = useState("");
  const [manualPort, setManualPort] = useState("1704");
  const [saving, setSaving] = useState(false);
  const [clientEnabled, setClientEnabled] = useState(true);
  const [serverRunning, setServerRunning] = useState(false);
  const [connectionMode, setConnectionMode] = useState<"auto" | "manual">("auto");
  const [testStatus, setTestStatus] = useState<"idle" | "testing" | "success" | "failed">("idle");
  
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
              if (checked) { await api.setClient(config); } else { await api.setClient({ ...config, server_url: "__disabled__" }); }
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
              {soundcards.length > 0 ? (
                <Select id={soundcardId} value={config.soundcard} onChange={(e) => setConfig({ ...config, soundcard: e.target.value })}>
                  <option value="default">{t("defaultSoundcard")}</option>
                  {soundcards.map((sc, i) => (<option key={i} value={`hw:${i}`}>{sc}</option>))}
                </Select>
              ) : (
                <Input id={soundcardId} value={config.soundcard} onChange={(e) => setConfig({ ...config, soundcard: e.target.value })} placeholder="default" />
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
  const tzId = useId();
  const cardId = useId();

  useEffect(() => {
    api.getTimezone().then((data) => {
      setTimezone(data.timezone);
      setAvailable(data.available);
    }).catch(() => {});
  }, []);

  if (!available.length) return null;

  return (
    <Card title={t("timezone")} id={cardId}>
      <Field label={t("timezoneSelect")} htmlFor={tzId}>
        <Select id={tzId} value={timezone} onChange={(e) => {
          setTimezone(e.target.value);
          api.setTimezone(e.target.value);
        }}>
          {available.map((tz) => (
            <option key={tz} value={tz}>{tz}</option>
          ))}
        </Select>
      </Field>
    </Card>
  );
}

function LogsCard() {
  const t = useTranslations("system");
  const [logs, setLogs] = useState<string[]>([]);
  const [expanded, setExpanded] = useState(false);
  const [filter, setFilter] = useState("all");
  const cardId = useId();
  const filterId = useId();

  const fetchLogs = useCallback(() => {
    const url = filter === "all" ? "/api/system/logs" : `/api/system/logs?service=${filter}`;
    fetch(url).then(r => r.json()).then((data) => {
      setLogs(data.lines || []);
    }).catch(() => {});
  }, [filter]);

  useEffect(() => { fetchLogs(); }, [fetchLogs]);

  return (
    <Card title={t("logs")} id={cardId}>
      <div className="space-y-2">
        <div className="flex flex-wrap items-end gap-2">
          <div className="flex gap-2">
            <Button variant="outline" size="sm" onClick={fetchLogs}>{t("refreshLogs")}</Button>
            <Button variant="outline" size="sm" onClick={() => setExpanded(!expanded)}>
              {expanded ? t("collapseLogs") : t("expandLogs")}
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
  const [channel, setChannel] = useState("stable");
  const [phase, setPhase] = useState<"idle" | "downloading" | "verifying" | "installing" | "rebooting" | "reconnecting" | "done" | "failed">("idle");
  const [rolledBack, setRolledBack] = useState(false);
  const channelId = useId();
  const cardId = useId();

  const fileInputRef = useRef<HTMLInputElement>(null);
  const [selectedFile, setSelectedFile] = useState<File | null>(null);
  const [showWarningGate, setShowWarningGate] = useState(false);
  const [acceptedRisks, setAcceptedRisks] = useState(false);
  const [uploadProgress, setUploadProgress] = useState(false);
  const [uploadError, setUploadError] = useState<string | null>(null);

  useEffect(() => {
    api.checkUpdate().then(setUpdate).catch(() => {});
    api.getUpdateStatus().then((s) => { if (s.rolled_back) setRolledBack(true); }).catch(() => {});
    api.getSystem().then((s) => setChannel(s.channel)).catch(() => {});
  }, []);

  const checkForUpdate = useCallback(() => {
    setChecking(true);
    api.checkUpdate().then(setUpdate).catch(() => {}).finally(() => setChecking(false));
  }, []);

  const performUpdate = useCallback(() => {
    if (!window.confirm(t("updateConfirm"))) return;
    setPhase("downloading");
    api.triggerUpdate().catch(() => { setPhase("failed"); });
    setTimeout(() => setPhase("verifying"), 3000);
    setTimeout(() => setPhase("installing"), 6000);
    setTimeout(() => {
      setPhase("rebooting");
      const startTime = Date.now();
      const poll = setInterval(async () => {
        if (Date.now() - startTime > 120000) { clearInterval(poll); setPhase("failed"); return; }
        try {
          setPhase("reconnecting");
          const sys = await api.getSystem();
          clearInterval(poll);
          if (update && sys.version === update.current_version) { setRolledBack(true); setPhase("failed"); }
          else { setPhase("done"); }
        } catch { /* still rebooting */ }
      }, 3000);
    }, 10000);
  }, [t, update]);

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
    setUploadProgress(true);
    setUploadError(null);
    api.uploadUpdate(selectedFile)
      .then(() => {
        setPhase("installing");
        setShowWarningGate(false);
        setSelectedFile(null);
        setAcceptedRisks(false);
        return api.installUpdate();
      })
      .then(() => {
        setTimeout(() => {
          setPhase("rebooting");
          const startTime = Date.now();
          const poll = setInterval(async () => {
            if (Date.now() - startTime > 120000) { clearInterval(poll); setPhase("failed"); return; }
            try {
              setPhase("reconnecting");
              await api.getSystem();
              clearInterval(poll);
              setPhase("done");
            } catch { /* still rebooting */ }
          }, 3000);
        }, 10000);
      })
      .catch((err) => {
        console.error(err);
        setUploadError(t("uploadError"));
        setUploadProgress(false);
      });
  }, [selectedFile, t]);

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
          <UpdatePhaseIndicator label={t(`phase_${phase}`)} />
        )}
        {phase === "done" && (
          <div className="rounded-lg bg-green-500/10 p-4 text-sm" role="status">
            <p className="font-medium text-green-700 dark:text-green-400">{t("updateSuccess")}</p>
          </div>
        )}
        {phase === "failed" && !rolledBack && (
          <div className="rounded-lg bg-destructive/10 p-4 text-sm" role="alert">
            <p className="font-medium text-destructive">{t("updateFailed")}</p>
          </div>
        )}

        {phase === "idle" && (
          <>
            {update?.available ? (
              <div className="flex flex-col gap-3 rounded-lg bg-primary/10 p-4">
                <div className="flex items-center justify-between">
                  <div>
                    <p className="text-sm font-medium">{t("updateAvailable")}</p>
                    <p className="text-xs text-muted-foreground">{update.current_version} → {update.latest_version}</p>
                  </div>
                  <Button size="sm" onClick={performUpdate}>{t("installUpdate")}</Button>
                </div>
                <div className="text-xs font-semibold flex items-center gap-1 border-t border-primary/20 pt-2 text-green-600 dark:text-green-400">
                  {update.signature_verified ? t("signatureVerified") : t("signatureUnverified")}
                </div>
              </div>
            ) : update?.is_downgrade ? (
              <div className="flex flex-col gap-3 rounded-lg bg-muted p-4">
                <div className="flex items-center justify-between">
                  <div>
                    <p className="text-sm font-medium">{t("downgradeAvailable")}</p>
                    <p className="text-xs text-muted-foreground">{update.current_version} → {update.latest_version}</p>
                  </div>
                  <Button variant="outline" size="sm" onClick={performUpdate}>{t("installVersion")}</Button>
                </div>
                <div className="text-xs font-semibold flex items-center gap-1 border-t border-border pt-2 text-green-600 dark:text-green-400">
                  {update.signature_verified ? t("signatureVerified") : t("signatureUnverified")}
                </div>
              </div>
            ) : update ? (
              <div className="flex flex-col gap-2 rounded-lg bg-muted/20 p-4">
                <div className="flex items-center gap-2 text-sm text-muted-foreground">
                  <StatusDot connected label={t("upToDate")} />
                  <span>{t("upToDate")}</span>
                </div>
                <div className="text-xs font-semibold flex items-center gap-1 text-green-600 dark:text-green-400 border-t border-border/50 pt-2">
                  {update.signature_verified ? t("signatureVerified") : t("signatureUnverified")}
                </div>
              </div>
            ) : null}
            <Button variant="outline" size="sm" onClick={checkForUpdate} disabled={checking} aria-busy={checking}>
              {checking ? t("checking") : t("checkNow")}
            </Button>
          </>
        )}

        <Field label={t("channel")} htmlFor={channelId}>
          <Select id={channelId} value={channel} onChange={(e) => { setChannel(e.target.value); api.setSystem({ channel: e.target.value }); }}>
            <option value="stable">{t("stable")}</option>
            <option value="beta">{t("beta")}</option>
          </Select>
        </Field>
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
                  accept=".tar.gz,.zip"
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
                disabled={uploadProgress}
              >
                {t("manualCancel")}
              </Button>
              <Button
                variant="destructive"
                onClick={startManualFlash}
                disabled={!acceptedRisks || uploadProgress}
                className="font-bold"
              >
                {uploadProgress ? t("manualUploading") : t("manualProceed")}
              </Button>
            </div>
          </div>
        </div>
      )}
    </Card>
  );
}

function AutoUpdateSettings() {
  const t = useTranslations("update");
  const [config, setConfig] = useState({ enabled: true, channel: "stable", interval: "daily", time: "04:00" });
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

  return (
    <div className="space-y-3 border-t border-border pt-3">
      <div className="flex items-center justify-between">
        <span className="text-sm text-muted-foreground">{t("autoUpdate")}</span>
        <Switch checked={config.enabled} onCheckedChange={(enabled) => save({ ...config, enabled })} />
      </div>
      {config.enabled && (
        <>
          <Field label={t("channel")} htmlFor={channelId}>
            <Select id={channelId} value={config.channel} onChange={(e) => save({ ...config, channel: e.target.value })}>
              <option value="stable">Stable</option>
              <option value="beta">Beta</option>
            </Select>
          </Field>
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

function SystemTab() {
  const t = useTranslations("system");
  const [info, setInfo] = useState<SystemInfo | null>(null);
  const cardId = useId();

  useEffect(() => {
    api.getSystem().then(setInfo).catch(() => {});
  }, []);

  if (!info) return <Skeleton className="h-32 w-full" aria-label={t("loading")} />;

  return (
    <div className="space-y-5">
      <DevicePasswordCard />
      <TimezoneCard />
      <LogsCard />
      <Card title={t("title")} id={cardId}>
        <div className="space-y-4">
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
    await api.setServer(config);
    setSaved(true);
    setTimeout(() => setSaved(false), 2000);
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
        <Select id={codecId} value={config.snapcast.codec} onChange={(e) => update("snapcast.codec", e.target.value)}>
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
        <Select id={bitDepthId} value={String(config.audio.bit_depth)} onChange={(e) => update("audio.bit_depth", Number(e.target.value))}>
          <option value="16">16</option>
          <option value="24">24</option>
          <option value="32">32</option>
        </Select>
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
            <Field label={t("password")} htmlFor={airPassId}><Input id={airPassId} value={config.airplay.password ?? ""} onChange={(e) => { const c = structuredClone(config); c.airplay = { ...c.airplay!, password: e.target.value || null }; setConfig(c); }} /></Field>
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

  const addZone = () => { const c = structuredClone(config); c.zones.push({ name: "", icon: "🔊" }); setConfig(c); };
  const removeZone = (i: number) => { const c = structuredClone(config); c.zones.splice(i, 1); setConfig(c); };
  const updateZone = (i: number, key: string, value: string) => { const c = structuredClone(config); (c.zones[i] as Record<string, string>)[key] = value; setConfig(c); };

  const addClient = () => { const c = structuredClone(config); c.clients.push({ name: "", mac: "", zone: config.zones[0]?.name ?? "", icon: "🔊", max_volume: 100 }); setConfig(c); };
  const removeClient = (i: number) => { const c = structuredClone(config); c.clients.splice(i, 1); setConfig(c); };
  const updateClient = (i: number, key: string, value: string | number) => { const c = structuredClone(config); (c.clients[i] as Record<string, string | number>)[key] = value; setConfig(c); };

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
    c.knx = on ? { role: "client", url: null, gos: [] } : null;
    setConfig(c);
  };
  const updateKnx = (key: string, value: string | null) => {
    const c = structuredClone(config);
    if (c.knx) (c.knx as Record<string, unknown>)[key] = value;
    setConfig(c);
  };

  const addGo = () => {
    const c = structuredClone(config);
    if (c.knx) { if (!c.knx.gos) c.knx.gos = []; c.knx.gos.push({ target: "", function: "", ga: "" }); }
    setConfig(c);
  };
  const removeGo = (i: number) => {
    const c = structuredClone(config);
    if (c.knx?.gos) c.knx.gos.splice(i, 1);
    setConfig(c);
  };
  const updateGo = (i: number, key: string, value: string) => {
    const c = structuredClone(config);
    if (c.knx?.gos) (c.knx.gos[i] as Record<string, string>)[key] = value;
    setConfig(c);
  };

  const knxFunctions = ["play", "pause", "next", "prev", "volume", "mute", "source"];
  const targets = [...config.zones.map((z) => z.name), ...config.clients.map((cl) => cl.name)];

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
          <div className="space-y-2 pl-2 border-l-2 border-border">
            <Field label={t("knxMode")} htmlFor={knxModeId}>
              <Select id={knxModeId} value={config.knx.role} onChange={(e) => updateKnx("role", e.target.value)}>
                <option value="client">{t("knxClient")}</option>
                <option value="device">{t("knxDevice")}</option>
              </Select>
            </Field>
            {config.knx.role === "client" && (
              <>
                <Field label={t("gatewayUrl")} htmlFor={knxUrlId}><Input id={knxUrlId} value={config.knx.url ?? ""} onChange={(e) => updateKnx("url", e.target.value || null)} /></Field>
                <div className="space-y-2">
                  <span className="text-xs font-medium text-muted-foreground">{t("knxGos")}</span>
                  {(config.knx.gos ?? []).map((go, i) => (
                    <div key={i} className="flex items-center gap-2">
                      <Select className="w-28" value={go.target} onChange={(e) => updateGo(i, "target", e.target.value)} aria-label={`${t("target")} ${i + 1}`}>
                        <option value="">—</option>
                        {targets.map((tgt) => <option key={tgt} value={tgt}>{tgt}</option>)}
                      </Select>
                      <Select className="w-24" value={go.function} onChange={(e) => updateGo(i, "function", e.target.value)} aria-label={`${t("function")} ${i + 1}`}>
                        <option value="">—</option>
                        {knxFunctions.map((f) => <option key={f} value={f}>{f}</option>)}
                      </Select>
                      <Input className="w-20" placeholder="x/x/x" value={go.ga} onChange={(e) => updateGo(i, "ga", e.target.value)} aria-label={`${t("ga")} ${i + 1}`} />
                      <Button variant="outline" size="icon-xs" onClick={() => removeGo(i)} aria-label="Remove">×</Button>
                    </div>
                  ))}
                  <Button variant="outline" size="xs" onClick={addGo}>{t("addGo")}</Button>
                </div>
              </>
            )}
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

function SetupPage() {
  const t = useTranslations("tabs");
  const systemT = useTranslations("system");
  const [tab, setTab] = useState<Tab>("dashboard");
  const { locale, setLocale } = useI18n();
  const [isConnected, setIsConnected] = useState(true);

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
      <main id="main-content" className="mx-auto w-full max-w-2xl px-4 py-6">
        <header className="mb-6 flex items-center gap-3">
          <img src="/icon.svg" alt="" className="size-10" aria-hidden="true" />
          <h1 className="flex-1 text-xl font-bold">{t("heading")}</h1>
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
        </header>
        <nav aria-label={t("navigation")}>
          <div className="mb-6 flex gap-1 overflow-x-auto rounded-xl bg-muted p-1" role="tablist" aria-label={t("navigation")}>
            {TABS.map((id) => (
              <button
                key={id}
                type="button"
                role="tab"
                id={`tab-${id}`}
                aria-selected={tab === id}
                aria-controls={`panel-${id}`}
                tabIndex={tab === id ? 0 : -1}
                className={`rounded-lg px-3 py-1.5 text-sm font-medium transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring ${
                  tab === id
                    ? "bg-card text-foreground shadow-sm"
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
