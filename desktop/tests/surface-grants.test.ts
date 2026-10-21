import { expect, test } from "bun:test";
import { available } from "../src/App";
import { SURFACES, surfaceById } from "../src/surfaces";
import { paths } from "../src/components/Icon";
import {
  ALL_APP_CAPABILITIES,
  DESKTOP_READ_CAPABILITIES,
} from "../src/ipc/generated.samples";
import type { RuntimeStatus, V3Capability } from "../src/ipc/generated";

/**
 * The surface grant gate.
 *
 * This is the one place that decides whether a capability the daemon
 * refused still shows a view. Two kinds of grant are in play: the
 * protocol-v1 status grant, and the protocol-v3 grant a surface needs when
 * its capability is v3-gated. A surface that mixes them up would either
 * hide a view the daemon supports, or show one it does not.
 */

const status: RuntimeStatus = {
  protocol_version: 1,
  schema_version: 1,
  edition: "standard",
  state: "ready",
  version: "2.6.0",
  build_git: "0f1e2d3",
  build_target: "aarch64-apple-darwin",
  capabilities: { capabilities: [...ALL_APP_CAPABILITIES] },
};

const v3Gated = DESKTOP_READ_CAPABILITIES.filter(
  (capability) => !(ALL_APP_CAPABILITIES as readonly string[]).includes(capability),
);

test("a surface with no capability is never available", () => {
  // The training and cluster surfaces require nothing, because there is
  // nothing behind them to require. They have their own unavailable views.
  for (const id of ["training", "cluster", "automations"]) {
    const surface = surfaceById(id);
    expect(surface.requires).toBeNull();
    expect(available(surface, status, [])).toBe(false);
  }
});

test("a protocol-v1 surface needs the status grant", () => {
  const doctor = surfaceById("doctor");
  expect(doctor.requires).toBe("read_status");
  expect(available(doctor, status, [])).toBe(true);

  // Without the status there is no status to read, so the surface is not
  // available however many v3 grants there are.
  const withoutStatus: RuntimeStatus = {
    ...status,
    capabilities: { capabilities: [] },
  };
  expect(available(doctor, withoutStatus, [])).toBe(false);
});

test("a surface is not available before the runtime has answered", () => {
  // No status yet means no grant yet. Showing the view and then failing
  // would be worse than not showing it.
  const doctor = surfaceById("doctor");
  expect(available(doctor, null, [])).toBe(false);
});

test("a v3-gated surface needs the v3 grant, not the status grant", () => {
  // The memory surface's capability is negotiated on the protocol-v3
  // session. A protocol-v1 status that happens to carry a similar name
  // must not open it.
  const memory = surfaceById("memory");
  expect(memory.requires).toBe("read_memory");
  expect(v3Gated).toContain("read_memory");

  // The status carries every v1 capability, and still the surface is shut.
  expect(available(memory, status, [])).toBe(false);

  // The v3 grant opens it.
  expect(available(memory, status, ["read_memory"])).toBe(true);
});

test("a v3-gated surface stays shut when only the status is present", () => {
  // This is the case the two grant kinds are easy to confuse: a status
  // that carries read_status says nothing about read_memory.
  for (const capability of v3Gated) {
    const surface = SURFACES.find((candidate) => candidate.requires === capability);
    if (surface === undefined) continue;
    expect(
      available(surface, status, []),
      `${capability} must not open on a protocol-v1 status alone`,
    ).toBe(false);
  }
});

test("a granted surface does not open on a different grant", () => {
  const memory = surfaceById("memory");
  // The v3 grant for something else is not the grant for this.
  expect(available(memory, status, ["read_models"])).toBe(false);
  expect(available(memory, status, ["read_models", "read_claims_by_id"])).toBe(false);
});

test("every available surface is gated by a capability that is declared", () => {
  // A surface requiring a capability nobody declares would be shut
  // forever, and the window would show nothing for it.
  const declared = new Set<string>([
    ...ALL_APP_CAPABILITIES,
    ...DESKTOP_READ_CAPABILITIES,
  ]);
  for (const surface of SURFACES) {
    if (surface.requires === null) continue;
    expect(
      declared.has(surface.requires),
      `${surface.id} requires undeclared ${surface.requires}`,
    ).toBe(true);
  }
});

test("every available surface opens when its own grant is held", () => {
  // The other direction: a surface whose grant is held must open. A
  // surface that could never open would be a broken navigation entry.
  for (const surface of SURFACES) {
    if (surface.requires === null) continue;
    const grants: V3Capability[] = v3Gated.includes(surface.requires)
      ? ([surface.requires] as V3Capability[])
      : [];
    expect(
      available(surface, status, grants),
      `${surface.id} must open on ${surface.requires}`,
    ).toBe(true);
  }
});

test("every surface has an icon", () => {
  // A surface with no icon renders the fallback spark, so the navigation
  // entry looks like a different feature. The check is against the real
  // icon table, so a surface added without an icon fails here.
  for (const surface of SURFACES) {
    expect(
      Object.prototype.hasOwnProperty.call(paths, surface.id),
      `${surface.id} has no icon`,
    ).toBe(true);
  }
});
