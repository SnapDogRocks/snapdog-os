# SnapDog server management contract

This document defines the lifecycle contract between `snapdog-ctrl`, the
SnapDog service, and the Control WebUI. It is intentionally stricter than the
systemd process state: the UI may say that the server is running only after the
application readiness check succeeds.

## State model

The following dimensions are independent:

- `setup_state`: `needs_setup`, `configured`, or `needs_repair`
- `desired_state`: `stopped` or `running`
- `config_state`: `missing`, `valid_unverified`, `valid`, `invalid`, or
  `unreadable`
- `runtime_state`: `stopped`, `starting`, `running`, `restarting`, `stopping`,
  `failed`, or `unknown`
- `health_state`: `unknown`, `checking`, `healthy`, or `unhealthy`
- `operation`: the currently serialized mutation and its phase, or `null`

`desired_state=running` with `runtime_state=failed` is a valid, important
state. It means the user asked for the server to be enabled, but the request
could not be fulfilled. The WebUI must show both facts instead of changing the
switch back or pretending that the service runs.

The server endpoint is offered only while `health_state=healthy`. A momentary
`systemctl is-active` result is never sufficient to set this state.

## First setup

An absent active configuration is represented as `needs_setup`; enabling the
service must not silently create and start an implicit default configuration.
The setup wizard creates an isolated draft and activates it only from the final
review step.

The normal path asks only for:

1. server name;
2. at least one zone;
3. optional known clients;
4. optional sources; and
5. review and start.

A client is a playback device. A zone is a synchronized playback group and can
represent one room, several rooms, a floor, or an outdoor area.

AirPlay is a built-in SnapDog source and is therefore always present. Setup may
choose AirPlay 2 (the default) or legacy AirPlay 1 and customize its password
and bind addresses. It must not present an enable switch for a source that the
server itself treats as mandatory.

## Applying existing configurations

All mutations are serialized by one server manager. An apply operation uses
this order:

1. verify the source revision;
2. render a candidate;
3. run structured validation and the installed `snapdog --check-config` guard;
4. perform environment preflight checks;
5. stage the candidate on the persistent data partition;
6. atomically activate it;
7. restart only when `desired_state=running`;
8. verify systemd state and application readiness;
9. mark the revision as last-known-good only after successful readiness; and
10. restore and re-verify the last-known-good revision after a failed runtime
    activation.

Saving while the desired state is stopped must never start the service. Such a
configuration remains `valid_unverified` until a successful start proves it.

Active config, candidate, last-known-good config, and the operation journal all
live below `/data/snapdog`. Atomic writes must not target `/etc/snapdog`, because
the read-only root filesystem contains only a symlink to the persistent file.
Source caches and other server-writable data live below `/data/snapdog/state`;
the service cannot write the configuration directory.

## Recovery

State and diagnostics endpoints remain available when the TOML cannot be
parsed. The config endpoint returns the raw source plus structured issues, so
the Advanced editor can repair the file.

An operation journal makes activation recoverable after power loss or a
controller crash. On startup, the manager reconciles an unfinished operation:

- a staged but inactive candidate is discarded;
- an activated but unverified candidate is checked and either committed or
  rolled back; and
- an interrupted rollback is resumed before another mutation is accepted.

Diagnostics may expose a bounded journal excerpt, service properties, and
probe results. Passwords, API keys, authorization headers, MQTT/Subsonic
credentials, and encryption PSKs must always be redacted.

Settings archives contain committed user configuration only. Transaction
artifacts, candidates, backups, the last-known-good revision, operation state,
and runtime data are never exported and are never accepted on import.

## UX invariants

- The WebUI never sets `running` optimistically.
- Config and state load independently; one failing must not hide the other.
- A dirty draft survives in-app navigation and is never overwritten by a
  WebSocket refresh.
- Structured editing and raw TOML editing cannot be active at the same time.
- Zone rename/delete operations update or explicitly resolve client and default
  zone references.
- Progress shows real phases, not invented percentages.
- Restart confirmation is required when active playback will be interrupted.
- Every terminal failure provides a next action and persists until resolved.
- Status is communicated by text and icon in addition to color.

## Acceptance checklist

Release validation must cover:

- new setup success and validation failure;
- enabled but failed service;
- save while stopped;
- successful guarded restart;
- failed activation with successful rollback;
- failed rollback;
- malformed TOML recovery;
- stale revision conflict without draft loss;
- a crash after initial readiness;
- controller restart during every durable operation phase; and
- desktop, mobile, keyboard, reduced-motion, and screen-reader interaction.
