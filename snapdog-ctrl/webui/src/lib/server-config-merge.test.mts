import assert from "node:assert/strict";
import test from "node:test";

// Node 26 runs this erasable TypeScript test directly; .mts makes the module
// format explicit and keeps the CI output free of module-detection warnings.
import type { ServerConfig } from "./api.ts";
import { mergeStructuredServerConfig } from "./server-config-merge.ts";

function fixture(): ServerConfig {
  return {
    revision: "old-revision",
    raw_toml: "old toml",
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
      default_zone: "Living Room",
    },
    mdns: { enabled: true, advertise_snapcast: false },
    dbus: { enabled: true },
    subsonic: null,
    spotify: null,
    airplay: { password: null, mode: "airplay2", bind: [] },
    mqtt: null,
    knx: null,
    zones: [
      {
        source_index: 0,
        name: "Living Room",
        icon: "🔊",
        sink: null,
        airplay_name: null,
        spotify_name: null,
        group_volume_mode: null,
        knx: null,
      },
      {
        source_index: 1,
        name: "Upper Floor",
        icon: "🏠",
        sink: null,
        airplay_name: null,
        spotify_name: null,
        group_volume_mode: null,
        knx: null,
      },
    ],
    clients: [
      {
        source_index: 0,
        name: "Kitchen",
        mac: "aa:bb:cc:dd:ee:01",
        zone: "Living Room",
        icon: "🔊",
        max_volume: 90,
        default_volume: 30,
        default_latency: 0,
        knx: null,
      },
      {
        source_index: 1,
        name: "Bedroom",
        mac: "aa:bb:cc:dd:ee:02",
        zone: "Upper Floor",
        icon: "🔊",
        max_volume: 80,
        default_volume: 25,
        default_latency: 0,
        knx: null,
      },
    ],
    radio: [{ source_index: 0, name: "News", url: "https://radio.example/news", cover: null }],
    system: { log_level: "info", log_file: null, state_dir: "/data/snapdog/state" },
  };
}

function clone(config: ServerConfig): ServerConfig {
  return structuredClone(config);
}

function merged(base: ServerConfig, draft: ServerConfig, fresh: ServerConfig): ServerConfig {
  const result = mergeStructuredServerConfig(base, draft, fresh);
  if (!result.ok) assert.fail(`Expected a successful merge: ${JSON.stringify(result.conflicts)}`);
  return result.value;
}

test("takes fresh values for fields the user did not touch", () => {
  const base = fixture();
  const draft = clone(base);
  const fresh = clone(base);
  fresh.name = "Changed elsewhere";
  fresh.revision = "fresh-revision";
  fresh.raw_toml = "fresh toml";

  const result = merged(base, draft, fresh);

  assert.equal(result.name, "Changed elsewhere");
  assert.equal(result.revision, "fresh-revision");
  assert.equal(result.raw_toml, "fresh toml");
});

test("keeps user-only changes and combines disjoint leaf changes", () => {
  const base = fixture();
  const draft = clone(base);
  const fresh = clone(base);
  draft.http.port = 6000;
  fresh.audio.sample_rate = 44100;

  const result = merged(base, draft, fresh);

  assert.equal(result.http.port, 6000);
  assert.equal(result.audio.sample_rate, 44100);
});

test("merges identical concurrent leaf changes", () => {
  const base = fixture();
  const draft = clone(base);
  const fresh = clone(base);
  draft.system.log_level = "debug";
  fresh.system.log_level = "debug";

  assert.equal(merged(base, draft, fresh).system.log_level, "debug");
});

test("reports a divergent edit to the same leaf without returning a partial value", () => {
  const base = fixture();
  const draft = clone(base);
  const fresh = clone(base);
  draft.http.port = 6000;
  fresh.http.port = 7000;

  const result = mergeStructuredServerConfig(base, draft, fresh);

  assert.equal(result.ok, false);
  if (result.ok) return;
  assert.deepEqual(result.conflicts, [{ path: "http.port", reason: "divergent_change" }]);
});

test("merges changes to different stable topology entries", () => {
  const base = fixture();
  const draft = clone(base);
  const fresh = clone(base);
  draft.zones[0].icon = "🎵";
  fresh.zones[1].sink = "alsa:upstairs";

  const result = merged(base, draft, fresh);

  assert.equal(result.zones[0].icon, "🎵");
  assert.equal(result.zones[1].sink, "alsa:upstairs");
});

test("uses source_index to follow a local rename while merging a remote edit", () => {
  const base = fixture();
  const draft = clone(base);
  const fresh = clone(base);
  draft.zones[1].name = "Upstairs";
  fresh.zones[1].sink = "alsa:upstairs";

  const result = merged(base, draft, fresh);

  assert.equal(result.zones[1].name, "Upstairs");
  assert.equal(result.zones[1].sink, "alsa:upstairs");
});

test("uses semantic identity when a remote deletion renumbers source indexes", () => {
  const base = fixture();
  const draft = clone(base);
  const fresh = clone(base);
  draft.zones[1].icon = "⬆️";
  fresh.zones = [{ ...fresh.zones[1], source_index: 0 }];

  const result = merged(base, draft, fresh);

  assert.equal(result.zones.length, 1);
  assert.equal(result.zones[0].name, "Upper Floor");
  assert.equal(result.zones[0].icon, "⬆️");
  assert.equal(result.zones[0].source_index, 0);
});

test("never treats a reused fresh source index as a stable topology identity", () => {
  const zoneBase = fixture();
  const zoneDraft = clone(zoneBase);
  const zoneFresh = clone(zoneBase);
  zoneDraft.zones[1].icon = "⬆️";
  zoneFresh.zones[1] = {
    ...zoneFresh.zones[1],
    source_index: 1,
    name: "Garden",
  };
  const zoneResult = mergeStructuredServerConfig(zoneBase, zoneDraft, zoneFresh);
  assert.equal(zoneResult.ok, false);
  if (!zoneResult.ok) assert.ok(zoneResult.conflicts.some((conflict) => conflict.reason === "ambiguous_identity"));

  const clientBase = fixture();
  const clientDraft = clone(clientBase);
  const clientFresh = clone(clientBase);
  clientDraft.clients[1].default_volume = 50;
  clientFresh.clients[1] = {
    ...clientFresh.clients[1],
    source_index: 1,
    name: "Terrace",
    mac: "aa:bb:cc:dd:ee:99",
  };
  const clientResult = mergeStructuredServerConfig(clientBase, clientDraft, clientFresh);
  assert.equal(clientResult.ok, false);
  if (!clientResult.ok) assert.ok(clientResult.conflicts.some((conflict) => conflict.reason === "ambiguous_identity"));

  const radioBase = fixture();
  const radioDraft = clone(radioBase);
  const radioFresh = clone(radioBase);
  radioDraft.radio[0].cover = "local.jpg";
  radioFresh.radio[0] = {
    source_index: 0,
    name: "Classical",
    url: "https://radio.example/classical",
    cover: null,
  };
  const radioResult = mergeStructuredServerConfig(radioBase, radioDraft, radioFresh);
  assert.equal(radioResult.ok, false);
  if (!radioResult.ok) assert.ok(radioResult.conflicts.some((conflict) => conflict.reason === "ambiguous_identity"));
});

test("rejects delete-versus-edit on the same topology entry", () => {
  const base = fixture();
  const draft = clone(base);
  const fresh = clone(base);
  draft.clients.splice(1, 1);
  fresh.clients[1].default_volume = 55;

  const result = mergeStructuredServerConfig(base, draft, fresh);

  assert.equal(result.ok, false);
  if (result.ok) return;
  assert.ok(result.conflicts.some((conflict) => conflict.path === "clients" && conflict.reason === "delete_vs_edit"));
});

test("preserves distinct concurrent appended entries", () => {
  const base = fixture();
  const draft = clone(base);
  const fresh = clone(base);
  draft.radio.push({ source_index: null, name: "Local", url: "https://radio.example/local", cover: null });
  fresh.radio.push({ source_index: 1, name: "Remote", url: "https://radio.example/remote", cover: null });

  const result = merged(base, draft, fresh);

  assert.deepEqual(result.radio.map((station) => station.name), ["News", "Remote", "Local"]);
});

test("recognizes an identical concurrent addition and keeps the fresh source index", () => {
  const base = fixture();
  const draft = clone(base);
  const fresh = clone(base);
  draft.radio.push({ source_index: null, name: "Jazz", url: "https://radio.example/jazz", cover: null });
  fresh.radio.push({ source_index: 1, name: "Jazz", url: "https://radio.example/jazz", cover: null });

  const result = merged(base, draft, fresh);

  assert.equal(result.radio.length, 2);
  assert.equal(result.radio[1].name, "Jazz");
  assert.equal(result.radio[1].source_index, 1);
});

test("rejects divergent changes to the same concurrent addition", () => {
  const base = fixture();
  const draft = clone(base);
  const fresh = clone(base);
  draft.radio.push({ source_index: null, name: "Jazz", url: "https://radio.example/jazz", cover: "local.jpg" });
  fresh.radio.push({ source_index: 1, name: "Jazz", url: "https://radio.example/jazz", cover: "remote.jpg" });

  const result = mergeStructuredServerConfig(base, draft, fresh);

  assert.equal(result.ok, false);
  if (result.ok) return;
  assert.ok(result.conflicts.some((conflict) => conflict.path === "radio.1.cover"));
});

test("rejects ambiguous identities and concurrent topology reordering", () => {
  const base = fixture();
  const draftWithUnknownIdentity = clone(base);
  const freshWithUnknownIdentity = clone(base);
  draftWithUnknownIdentity.zones.push({ source_index: null, name: "", icon: "", sink: null, airplay_name: null, spotify_name: null, group_volume_mode: null, knx: null });
  freshWithUnknownIdentity.zones[0].sink = "alsa:living-room";
  const identityResult = mergeStructuredServerConfig(base, draftWithUnknownIdentity, freshWithUnknownIdentity);
  assert.equal(identityResult.ok, false);
  if (!identityResult.ok) assert.ok(identityResult.conflicts.some((conflict) => conflict.reason === "ambiguous_identity"));

  const reorderedDraft = clone(base);
  const editedFresh = clone(base);
  reorderedDraft.zones.reverse();
  editedFresh.zones[0].sink = "alsa:living-room";
  const orderResult = mergeStructuredServerConfig(base, reorderedDraft, editedFresh);
  assert.equal(orderResult.ok, false);
  if (!orderResult.ok) assert.ok(orderResult.conflicts.some((conflict) => conflict.reason === "ambiguous_order"));
});

test("rejects different concurrent changes to scalar arrays", () => {
  const base = fixture();
  const draft = clone(base);
  const fresh = clone(base);
  draft.airplay.bind = ["192.0.2.10"];
  fresh.airplay.bind = ["192.0.2.20"];

  const result = mergeStructuredServerConfig(base, draft, fresh);

  assert.equal(result.ok, false);
  if (!result.ok) assert.deepEqual(result.conflicts, [{ path: "airplay.bind", reason: "divergent_change" }]);
});

test("does not mutate any input on success or conflict", () => {
  const base = fixture();
  const draft = clone(base);
  const fresh = clone(base);
  draft.http.port = 6000;
  fresh.audio.channels = 4;
  const snapshots = [clone(base), clone(draft), clone(fresh)];
  mergeStructuredServerConfig(base, draft, fresh);
  assert.deepEqual([base, draft, fresh], snapshots);

  fresh.http.port = 7000;
  const conflictSnapshots = [clone(base), clone(draft), clone(fresh)];
  mergeStructuredServerConfig(base, draft, fresh);
  assert.deepEqual([base, draft, fresh], conflictSnapshots);
});
