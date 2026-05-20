"use client";

import { useState, useEffect, useCallback, useId } from "react";
import { useTranslations } from "next-intl";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Skeleton } from "@/components/ui/skeleton";
import { Select } from "@/components/ui/select";
import { Switch } from "@/components/ui/switch";
import {
  api,
  type SystemInfo,
  type WifiNetwork,
  type NetworkConfig,
  type AudioConfig,
  type ClientConfig,
  type SshConfig,
  type DacOverlay,
} from "@/lib/api";
import { useI18n } from "@/i18n/provider";
import { locales, type Locale } from "@/i18n/config";

type Tab = "dashboard" | "network" | "audio" | "client" | "ssh" | "update" | "system";

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
            Client {info.components.client} · Ctrl {info.components.ctrl} · Kernel {info.components.kernel}
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
  const [scanning, setScanning] = useState(false);
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
    scan();
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
  }, [scan]);

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
    </div>
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
  const [scanning, setScanning] = useState(false);
  const [manualHost, setManualHost] = useState("");
  const [manualPort, setManualPort] = useState("1704");
  const [saving, setSaving] = useState(false);
  const hostIdFieldId = useId();
  const soundcardId = useId();
  const mixerId = useId();
  const latencyId = useId();
  const cardId = useId();

  useEffect(() => {
    api.getClient().then((c) => {
      setConfig(c);
      if (c.available_soundcards) setSoundcards(c.available_soundcards);
      // Parse existing manual URL
      const match = c.server_url.match(/^tcp:\/\/(.+):(\d+)$/);
      if (match) { setManualHost(match[1]); setManualPort(match[2]); }
    }).catch(() => {});
    scanForServers();
  }, []);

  const scanForServers = useCallback(() => {
    setScanning(true);
    api.scanServers().then((r) => setServers(r.servers)).catch(() => {}).finally(() => setScanning(false));
  }, []);

  const selectServer = (url: string) => {
    setConfig({ ...config, server_url: url });
    setManualHost("");
    setManualPort("1704");
  };

  const selectedUrl = manualHost ? `tcp://${manualHost}:${manualPort}` : config.server_url;

  const saveConfig = useCallback(async () => {
    const url = manualHost ? `tcp://${manualHost}:${manualPort}` : config.server_url;
    if (url) {
      const host = manualHost || url.replace(/^tcp:\/\//, "").split(":")[0];
      const port = Number(manualPort) || 1704;
      setSaving(true);
      try {
        const result = await api.testServer(host, port);
        if (!result.reachable && !window.confirm(t("serverUnreachable"))) { setSaving(false); return; }
      } catch {
        if (!window.confirm(t("serverTestFailed"))) { setSaving(false); return; }
      }
    }
    await api.setClient({ ...config, server_url: manualHost ? `tcp://${manualHost}:${manualPort}` : config.server_url });
    setSaving(false);
  }, [config, manualHost, manualPort, t]);

  return (
    <Card title={t("title")} id={cardId}>
      <div className="space-y-4">
        {/* Server selection — Apple-style list */}
        <div>
          <div className="mb-1.5 flex items-center justify-between">
            <span className="text-sm text-muted-foreground">{t("server")}</span>
            <button type="button" onClick={scanForServers} disabled={scanning} className="text-xs text-primary hover:underline disabled:opacity-50">
              {scanning ? t("scanning") : t("scanServers")}
            </button>
          </div>
          <div className="overflow-hidden rounded-xl border border-border">
            {/* Auto option */}
            <button
              type="button"
              className={`flex w-full items-center justify-between border-b border-border px-3 py-2.5 text-left text-sm transition-colors ${!selectedUrl ? "bg-primary/10 font-medium" : "hover:bg-muted"}`}
              onClick={() => selectServer("")}
            >
              <span>{t("autoDiscover")}</span>
              {!selectedUrl && <span className="text-primary">✓</span>}
            </button>
            {/* Discovered servers */}
            {servers.map((s) => {
              const url = `tcp://${s.host}:${s.port}`;
              const isSelected = selectedUrl === url;
              return (
                <button
                  key={s.host}
                  type="button"
                  className={`flex w-full items-center justify-between border-b border-border px-3 py-2.5 text-left text-sm transition-colors ${isSelected ? "bg-primary/10 font-medium" : "hover:bg-muted"}`}
                  onClick={() => selectServer(url)}
                >
                  <div>
                    <span>{s.name}</span>
                    <span className="ml-2 text-xs text-muted-foreground">{s.host}</span>
                  </div>
                  {isSelected && <span className="text-primary">✓</span>}
                </button>
              );
            })}
            {/* Manual entry — always visible at bottom */}
            <div className="flex items-center gap-2 px-3 py-2.5">
              <Input
                value={manualHost}
                onChange={(e) => { setManualHost(e.target.value); if (e.target.value) setConfig({ ...config, server_url: "" }); }}
                placeholder={t("manualPlaceholder")}
                className="h-7 flex-1 text-sm"
                aria-label={t("serverAddress")}
              />
              <span className="text-xs text-muted-foreground">:</span>
              <Input
                value={manualPort}
                onChange={(e) => setManualPort(e.target.value)}
                className="h-7 w-16 text-sm"
                aria-label={t("port")}
              />
            </div>
          </div>
        </div>

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
    fetch("/api/system/timezone").then(r => r.json()).then((data) => {
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
          fetch("/api/system/timezone", { method: "PUT", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ timezone: e.target.value }) });
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
  const cardId = useId();

  const fetchLogs = useCallback(() => {
    fetch("/api/system/logs").then(r => r.json()).then((data) => {
      setLogs(data.lines || []);
    }).catch(() => {});
  }, []);

  useEffect(() => { fetchLogs(); }, [fetchLogs]);

  return (
    <Card title={t("logs")} id={cardId}>
      <div className="space-y-2">
        <div className="flex gap-2">
          <Button variant="outline" size="sm" onClick={fetchLogs}>{t("refreshLogs")}</Button>
          <Button variant="outline" size="sm" onClick={() => setExpanded(!expanded)}>
            {expanded ? t("collapseLogs") : t("expandLogs")}
          </Button>
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
              <div className="flex items-center justify-between rounded-lg bg-primary/10 p-4">
                <div>
                  <p className="text-sm font-medium">{t("updateAvailable")}</p>
                  <p className="text-xs text-muted-foreground">{update.current_version} → {update.latest_version}</p>
                </div>
                <Button size="sm" onClick={performUpdate}>{t("installUpdate")}</Button>
              </div>
            ) : update?.is_downgrade ? (
              <div className="flex items-center justify-between rounded-lg bg-muted p-4">
                <div>
                  <p className="text-sm font-medium">{t("downgradeAvailable")}</p>
                  <p className="text-xs text-muted-foreground">{update.current_version} → {update.latest_version}</p>
                </div>
                <Button variant="outline" size="sm" onClick={performUpdate}>{t("installVersion")}</Button>
              </div>
            ) : update ? (
              <div className="flex items-center gap-2 text-sm text-muted-foreground">
                <StatusDot connected label={t("upToDate")} />
                <span>{t("upToDate")}</span>
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
      </div>
    </Card>
  );
}

function AutoUpdateSettings() {
  const t = useTranslations("update");
  const [config, setConfig] = useState({ enabled: true, interval: "daily", time: "03:00" });
  const intervalId = useId();
  const timeId = useId();

  useEffect(() => {
    fetch("/api/system/update/auto").then(r => r.json()).then(setConfig).catch(() => {});
  }, []);

  const save = (updated: typeof config) => {
    setConfig(updated);
    fetch("/api/system/update/auto", { method: "PUT", headers: { "Content-Type": "application/json" }, body: JSON.stringify(updated) });
  };

  return (
    <div className="space-y-3 border-t border-border pt-3">
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


// ── Main Page ─────────────────────────────────────────────────

const TABS: Tab[] = ["dashboard", "network", "audio", "client", "ssh", "update", "system"];

export default function SetupPage() {
  const t = useTranslations("tabs");
  const [tab, setTab] = useState<Tab>("dashboard");
  const { locale, setLocale } = useI18n();

  return (
    <>
      <a href="#main-content" className="sr-only focus:not-sr-only focus:absolute focus:left-4 focus:top-4 focus:z-50 focus:rounded-lg focus:bg-primary focus:px-4 focus:py-2 focus:text-primary-foreground">
        {t("skipToContent")}
      </a>
      <main id="main-content" className="mx-auto w-full max-w-2xl px-4 py-6">
        <header className="mb-6 flex items-center gap-3">
          <img src="/icon.svg" alt="" className="size-10" aria-hidden="true" />
          <h1 className="flex-1 text-xl font-bold">{t("heading")}</h1>
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
          {tab === "ssh" && <SshTab />}
          {tab === "update" && <UpdateTab />}
          {tab === "system" && <SystemTab />}
        </div>
      </main>
    </>
  );
}
