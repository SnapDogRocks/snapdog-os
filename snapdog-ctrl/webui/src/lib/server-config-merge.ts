import type { ServerConfig } from "./api";

export type ServerConfigMergeConflictReason =
  | "divergent_change"
  | "delete_vs_edit"
  | "ambiguous_identity"
  | "ambiguous_order";

export interface ServerConfigMergeConflict {
  path: string;
  reason: ServerConfigMergeConflictReason;
}

export type ServerConfigMergeResult =
  | { ok: true; value: ServerConfig }
  | { ok: false; conflicts: ServerConfigMergeConflict[] };

type TopologyKind = "zones" | "clients" | "radio";
type MergeNode = unknown | typeof MISSING;
type MergeSide = "draft" | "fresh";

interface Identity {
  kind: string;
  value: string;
}

interface BaseRecord {
  id: string;
  index: number;
  value: Record<string, unknown>;
}

interface BranchRecord {
  id: string;
  index: number;
  value: Record<string, unknown>;
  identities: Identity[];
}

interface MergeContext {
  conflicts: ServerConfigMergeConflict[];
}

const MISSING = Symbol("missing config value");
const TOPOLOGY_PATHS = new Set<TopologyKind>(["zones", "clients", "radio"]);
const FRESH_METADATA_KEYS = new Set(["revision", "raw_toml", "raw_toml_changed"]);

function isObject(value: MergeNode): value is Record<string, unknown> {
  return value !== MISSING && value !== null && typeof value === "object" && !Array.isArray(value);
}

function cloneNode<T extends MergeNode>(value: T): T {
  return value === MISSING ? value : structuredClone(value);
}

function equalNode(left: MergeNode, right: MergeNode): boolean {
  if (left === right) return true;
  if (left === MISSING || right === MISSING) return false;
  if (left === null || right === null || typeof left !== "object" || typeof right !== "object") return false;
  if (Array.isArray(left) || Array.isArray(right)) {
    if (!Array.isArray(left) || !Array.isArray(right) || left.length !== right.length) return false;
    return left.every((value, index) => equalNode(value, right[index]));
  }
  if (!isObject(left) || !isObject(right)) return false;
  const leftKeys = Object.keys(left);
  const rightKeys = Object.keys(right);
  if (leftKeys.length !== rightKeys.length) return false;
  return leftKeys.every((key) => Object.hasOwn(right, key) && equalNode(left[key], right[key]));
}

function addConflict(context: MergeContext, path: string, reason: ServerConfigMergeConflictReason): void {
  const normalizedPath = path || "configuration";
  if (!context.conflicts.some((conflict) => conflict.path === normalizedPath && conflict.reason === reason)) {
    context.conflicts.push({ path: normalizedPath, reason });
  }
}

function normalizedIdentity(value: unknown): string | null {
  if (typeof value !== "string") return null;
  const normalized = value.trim().toLocaleLowerCase();
  return normalized || null;
}

function identitiesFor(kind: TopologyKind, value: Record<string, unknown>): Identity[] {
  const identities: Identity[] = [];
  const add = (identityKind: string, candidate: unknown) => {
    const normalized = normalizedIdentity(candidate);
    if (normalized) identities.push({ kind: identityKind, value: normalized });
  };

  if (kind === "zones") {
    add("name", value.name);
  } else if (kind === "clients") {
    add("mac", value.mac);
    add("name", value.name);
  } else {
    add("url", value.url);
    add("name", value.name);
  }
  return identities;
}

function identityToken(identity: Identity): string {
  return `${identity.kind}:${identity.value}`;
}

function additionIdentity(kind: TopologyKind, identities: Identity[]): string | null {
  const preferredKind = kind === "clients" ? "mac" : kind === "radio" ? "url" : "name";
  const identity = identities.find((candidate) => candidate.kind === preferredKind) ?? identities[0];
  return identity ? `addition:${identityToken(identity)}` : null;
}

function sourceIndex(value: Record<string, unknown>): number | null {
  return typeof value.source_index === "number" && Number.isInteger(value.source_index)
    ? value.source_index
    : null;
}

function buildBaseRecords(
  kind: TopologyKind,
  values: unknown[],
  path: string,
  context: MergeContext,
): {
  records: BaseRecord[];
  byId: Map<string, BaseRecord>;
  bySource: Map<number, string[]>;
  byIdentity: Map<string, string[]>;
} | null {
  const records: BaseRecord[] = [];
  const byId = new Map<string, BaseRecord>();
  const bySource = new Map<number, string[]>();
  const byIdentity = new Map<string, string[]>();

  for (const [index, value] of values.entries()) {
    if (!isObject(value)) {
      addConflict(context, path, "ambiguous_identity");
      return null;
    }
    const record: BaseRecord = { id: `base:${index}`, index, value };
    records.push(record);
    byId.set(record.id, record);

    const source = sourceIndex(value);
    if (source !== null) {
      const ids = bySource.get(source) ?? [];
      ids.push(record.id);
      bySource.set(source, ids);
    }
    for (const identity of identitiesFor(kind, value)) {
      const token = identityToken(identity);
      const ids = byIdentity.get(token) ?? [];
      ids.push(record.id);
      byIdentity.set(token, ids);
    }
  }
  return { records, byId, bySource, byIdentity };
}

function matchBranchRecords(
  kind: TopologyKind,
  values: unknown[],
  side: MergeSide,
  base: NonNullable<ReturnType<typeof buildBaseRecords>>,
  path: string,
  context: MergeContext,
): BranchRecord[] | null {
  const records: BranchRecord[] = [];
  const usedIds = new Set<string>();

  for (const [index, value] of values.entries()) {
    if (!isObject(value)) {
      addConflict(context, path, "ambiguous_identity");
      return null;
    }
    const identities = identitiesFor(kind, value);
    const semanticMatches = new Set<string>();
    for (const identity of identities) {
      for (const id of base.byIdentity.get(identityToken(identity)) ?? []) semanticMatches.add(id);
    }
    const source = sourceIndex(value);
    const sourceMatches = source === null ? [] : (base.bySource.get(source) ?? []);
    let id: string | null = null;

    if (side === "draft" && sourceMatches.length === 1) {
      const sourceId = sourceMatches[0];
      if ([...semanticMatches].some((candidate) => candidate !== sourceId)) {
        addConflict(context, `${path}.${index}`, "ambiguous_identity");
        return null;
      }
      id = sourceId;
    } else if (semanticMatches.size === 1) {
      id = [...semanticMatches][0];
    } else if (semanticMatches.size > 1) {
      addConflict(context, `${path}.${index}`, "ambiguous_identity");
      return null;
    } else if (side === "fresh" && sourceMatches.length > 0) {
      // source_index is a position in the newly parsed document, not a durable
      // identity. After a delete/insert the same number can describe a wholly
      // different entry. A stable semantic match above may confirm an existing
      // entry; source-only matching on the fresh side must stay conservative.
      addConflict(context, `${path}.${index}`, "ambiguous_identity");
      return null;
    } else if (sourceMatches.length === 1) {
      id = sourceMatches[0];
    } else if (sourceMatches.length > 1) {
      addConflict(context, `${path}.${index}`, "ambiguous_identity");
      return null;
    } else {
      id = additionIdentity(kind, identities);
      if (!id) {
        addConflict(context, `${path}.${index}`, "ambiguous_identity");
        return null;
      }
    }

    if (usedIds.has(id)) {
      addConflict(context, `${path}.${index}`, "ambiguous_identity");
      return null;
    }
    usedIds.add(id);
    records.push({ id, index, value, identities });
  }
  return records;
}

function hasIdentityOverlap(left: BranchRecord, right: BranchRecord): boolean {
  const leftTokens = new Set(left.identities.map(identityToken));
  return right.identities.some((identity) => leftTokens.has(identityToken(identity)));
}

function reorderedBaseItems(records: BranchRecord[], baseIds: string[]): boolean {
  const actual = records.map((record) => record.id).filter((id) => id.startsWith("base:"));
  const present = new Set(actual);
  const expected = baseIds.filter((id) => present.has(id));
  return !equalNode(actual, expected);
}

function hasInterleavedAddition(records: BranchRecord[]): boolean {
  let sawAddition = false;
  for (const record of records) {
    if (!record.id.startsWith("base:")) sawAddition = true;
    else if (sawAddition) return true;
  }
  return false;
}

function mergeTopologyArray(
  kind: TopologyKind,
  baseValues: unknown[],
  draftValues: unknown[],
  freshValues: unknown[],
  path: string,
  context: MergeContext,
): unknown[] {
  const base = buildBaseRecords(kind, baseValues, path, context);
  if (!base) return cloneNode(draftValues);
  const draft = matchBranchRecords(kind, draftValues, "draft", base, path, context);
  const fresh = matchBranchRecords(kind, freshValues, "fresh", base, path, context);
  if (!draft || !fresh) return cloneNode(draftValues);

  const baseIds = base.records.map((record) => record.id);
  if (reorderedBaseItems(draft, baseIds) || reorderedBaseItems(fresh, baseIds)
    || hasInterleavedAddition(draft) || hasInterleavedAddition(fresh)) {
    addConflict(context, path, "ambiguous_order");
    return cloneNode(draftValues);
  }

  const draftAdditions = draft.filter((record) => !record.id.startsWith("base:"));
  const freshAdditions = fresh.filter((record) => !record.id.startsWith("base:"));
  for (const draftRecord of draftAdditions) {
    for (const freshRecord of freshAdditions) {
      if (draftRecord.id !== freshRecord.id && hasIdentityOverlap(draftRecord, freshRecord)) {
        addConflict(context, path, "ambiguous_identity");
        return cloneNode(draftValues);
      }
    }
  }

  const draftById = new Map(draft.map((record) => [record.id, record]));
  const freshById = new Map(fresh.map((record) => [record.id, record]));
  const mergedById = new Map<string, MergeNode>();
  const allIds = new Set([
    ...baseIds,
    ...draft.map((record) => record.id),
    ...fresh.map((record) => record.id),
  ]);

  for (const id of allIds) {
    const baseRecord = base.byId.get(id);
    const draftRecord = draftById.get(id);
    const freshRecord = freshById.get(id);
    // Conflict paths must point into the draft the user can actually edit. If
    // the draft deleted an entry, surface the array itself instead of pointing
    // at a same-numbered (but potentially different) local item.
    const displayIndex = draftRecord?.index;
    const itemPath = displayIndex === undefined ? path : `${path}.${displayIndex}`;
    mergedById.set(id, mergeNode(
      baseRecord?.value ?? MISSING,
      draftRecord?.value ?? MISSING,
      freshRecord?.value ?? MISSING,
      itemPath,
      context,
    ));
  }

  const result: unknown[] = [];
  const emitted = new Set<string>();
  for (const record of fresh) {
    const value = mergedById.get(record.id);
    if (value !== undefined && value !== MISSING) result.push(value);
    emitted.add(record.id);
  }
  // The editor only appends new topology entries. Keep the server's current
  // order intact, then append additions that exist only in the local draft.
  for (const record of draft) {
    if (emitted.has(record.id)) continue;
    const value = mergedById.get(record.id);
    if (value !== undefined && value !== MISSING) result.push(value);
    emitted.add(record.id);
  }
  return result;
}

function mergeNode(
  base: MergeNode,
  draft: MergeNode,
  fresh: MergeNode,
  path: string,
  context: MergeContext,
): MergeNode {
  if (equalNode(draft, base)) return cloneNode(fresh);
  if (equalNode(fresh, base)) return cloneNode(draft);
  if (equalNode(draft, fresh)) return cloneNode(draft);

  // source_index is parser metadata, not an editable field. When an entry was
  // independently added on both sides, use the index from the active revision.
  if (path.endsWith(".source_index") && fresh !== MISSING) return cloneNode(fresh);

  if (base === MISSING && isObject(draft) && isObject(fresh)) {
    const result: Record<string, unknown> = {};
    const keys = new Set([...Object.keys(draft), ...Object.keys(fresh)]);
    for (const key of keys) {
      const value = mergeNode(
        MISSING,
        Object.hasOwn(draft, key) ? draft[key] : MISSING,
        Object.hasOwn(fresh, key) ? fresh[key] : MISSING,
        path ? `${path}.${key}` : key,
        context,
      );
      if (value !== MISSING) result[key] = value;
    }
    return result;
  }

  if (base === MISSING || draft === MISSING || fresh === MISSING) {
    addConflict(context, path, base === MISSING ? "divergent_change" : "delete_vs_edit");
    return cloneNode(draft);
  }

  if (Array.isArray(base) && Array.isArray(draft) && Array.isArray(fresh)) {
    if (TOPOLOGY_PATHS.has(path as TopologyKind)) {
      return mergeTopologyArray(path as TopologyKind, base, draft, fresh, path, context);
    }
    // Scalar lists (API keys, AirPlay bind addresses, …) have no durable item
    // identity. Fast paths above handle one-sided and identical changes; two
    // different concurrent edits must be resolved explicitly.
    addConflict(context, path, "divergent_change");
    return cloneNode(draft);
  }

  if (isObject(base) && isObject(draft) && isObject(fresh)) {
    const result: Record<string, unknown> = {};
    const keys = new Set([...Object.keys(base), ...Object.keys(draft), ...Object.keys(fresh)]);
    for (const key of keys) {
      const childPath = path ? `${path}.${key}` : key;
      if (!path && FRESH_METADATA_KEYS.has(key)) {
        const freshValue = Object.hasOwn(fresh, key) ? fresh[key] : MISSING;
        if (freshValue !== MISSING) result[key] = cloneNode(freshValue);
        continue;
      }
      const value = mergeNode(
        Object.hasOwn(base, key) ? base[key] : MISSING,
        Object.hasOwn(draft, key) ? draft[key] : MISSING,
        Object.hasOwn(fresh, key) ? fresh[key] : MISSING,
        childPath,
        context,
      );
      if (value !== MISSING) result[key] = value;
    }
    return result;
  }

  addConflict(context, path, "divergent_change");
  return cloneNode(draft);
}

/**
 * Merge a guided editor draft onto a newer active server configuration.
 *
 * The inputs are never mutated. A failed result deliberately contains no
 * partial value so callers cannot accidentally install an ambiguous merge.
 */
export function mergeStructuredServerConfig(
  oldBase: ServerConfig,
  userDraft: ServerConfig,
  freshBase: ServerConfig,
): ServerConfigMergeResult {
  const context: MergeContext = { conflicts: [] };
  const value = mergeNode(oldBase, userDraft, freshBase, "", context);
  if (context.conflicts.length > 0 || !isObject(value)) {
    return { ok: false, conflicts: context.conflicts };
  }
  return { ok: true, value: value as unknown as ServerConfig };
}
