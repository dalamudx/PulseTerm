import { afterEach, expect, it, vi } from "vitest";
import type { KeyIdentityCatalogue } from "./workspace-key-identities";
import { WorkspaceSshDialog } from "./workspace-ssh-dialog";
import type {
  RouteDecision,
  RoutePrompt,
  RouteSelection,
} from "./workspace-ssh-route";
import type { SavedSshSelection } from "./workspace-ssh-session";

const nonce = "11111111-1111-4111-8111-111111111111",
  hostId = "11111111-1111-7111-8111-111111111111",
  identityId = "22222222-2222-7222-8222-222222222222";
class Node extends EventTarget {
  children: Node[] = [];
  textContent = "";
  value = "";
  disabled = false;
  open = false;
  isConnected = true;
  focuses = 0;
  ownerDocument: {
    activeElement: Node | null;
    createElement: (tag: string) => Node;
  };
  constructor(
    readonly tag = "div",
    doc?: Node["ownerDocument"],
  ) {
    super();
    this.ownerDocument = doc ?? {
      activeElement: null,
      createElement: (tag) => new Node(tag, this.ownerDocument),
    };
  }
  append(node: Node) {
    this.children.push(node);
    if (this.tag === "select" && !this.value && !node.disabled)
      this.value = node.value;
  }
  get options() {
    return this.children;
  }
  get selectedOptions() {
    return this.children.filter((n) => n.value === this.value);
  }
  set selectedIndex(index: number) {
    this.value = this.children[index]?.value ?? "";
  }
  replaceChildren(...nodes: Node[]) {
    this.children = nodes;
    this.value = "";
  }
  showModal() {
    this.open = true;
  }
  close() {
    this.open = false;
  }
  focus() {
    this.focuses++;
    this.ownerDocument.activeElement = this;
  }
}
function deferred<T>() {
  let resolve: (value: T) => void = () => {};
  let reject: (reason?: unknown) => void = () => {};
  const promise = new Promise<T>((done, fail) => {
    resolve = done;
    reject = fail;
  });
  return { promise, resolve, reject };
}
function fixture() {
  vi.stubGlobal("HTMLElement", Node);
  const origin = new Node("button"),
    doc = origin.ownerDocument;
  doc.activeElement = origin;
  const dialog = new Node("dialog", doc),
    host = new Node("select", doc),
    identity = new Node("select", doc),
    connect = new Node("button", doc),
    cancel = new Node("button", doc),
    status = new Node("p", doc),
    selectedHost = new Node("output", doc);
  const life = new AbortController();
  let current = nonce;
  const hosts = {
    version: 1 as const,
    token: nonce,
    revision: "1",
    hosts: [
      {
        id: hostId,
        label: "<img src=x>",
        host: "127.0.0.1",
        port: 22,
        username: "u",
      },
    ],
  };
  const identities = {
    version: 1 as const,
    token: nonce,
    revision: "1",
    vaultPresent: true,
    identities: [
      {
        id: identityId,
        label: "fixed",
        username: "user",
        secret: { id: nonce, kind: "ssh_password" as const, hasValue: true },
      },
    ],
  };
  const keys: KeyIdentityCatalogue = {
    version: 1,
    token: nonce,
    revision: "1",
    vaultPresent: true,
    identities: [],
  };
  const dependencies = {
    hosts: vi.fn(async () => hosts),
    identities: vi.fn(async () => identities),
    keyIdentities: vi.fn(async () => keys),
    agentIdentities: vi.fn(
      async () =>
        ({
          version: 1 as const,
          token: nonce,
          revision: "0",
          identities: [],
          keys: [],
        }) as import("./workspace-agent-identities").AgentCatalogue,
    ),
    continueRoute: vi.fn(
      async (
        _nonce: string,
        _token: string,
        _signal: AbortSignal,
        _current: () => boolean,
        _confirm: (prompt: RoutePrompt) => Promise<RouteDecision>,
      ) => "3",
    ),
    connectAgent: vi.fn(async () => "8"),
    connectAgentInteractive: vi.fn(async (..._args: unknown[]) => "11"),
    connectInteractive: vi.fn(
      async (
        _nonce: string,
        _selection: SavedSshSelection,
        _signal: AbortSignal,
        _current: () => boolean,
      ) => "9",
    ),
    connectRoute: vi.fn(
      async (
        _nonce: string,
        _selection: RouteSelection,
        _signal: AbortSignal,
        _current: () => boolean,
        _confirm: (prompt: RoutePrompt) => Promise<RouteDecision>,
      ) => "3",
    ),
    connectKeyInteractive: vi.fn(async (..._args: unknown[]) => "10"),
    connectPasswordInteractive: vi.fn(async (..._args: unknown[]) => "12"),
    connectKey: vi.fn(
      async (
        _nonce: string,
        _selection: SavedSshSelection,
        _signal: AbortSignal,
        _current: () => boolean,
      ) => "2",
    ),
    connect: vi.fn(
      async (
        _nonce: string,
        _selection: SavedSshSelection,
        _signal: AbortSignal,
        _current: () => boolean,
      ) => "1",
    ),
  };
  const working = vi.fn(),
    connected = vi.fn(async () => {}),
    temporaryConnected = vi.fn();
  const controller = new WorkspaceSshDialog(
    {
      dialog: dialog as unknown as HTMLDialogElement,
      host: host as unknown as HTMLSelectElement,
      identity: identity as unknown as HTMLSelectElement,
      connect: connect as unknown as HTMLButtonElement,
      cancel: cancel as unknown as HTMLButtonElement,
      status: status as unknown as HTMLElement,
      selectedHost: selectedHost as unknown as HTMLElement,
      signal: life.signal,
      nonce: () => current,
      working,
      connected,
      temporaryConnected,
    },
    dependencies,
  );
  return {
    controller,
    dialog,
    host,
    identity,
    connect,
    cancel,
    status,
    selectedHost,
    life,
    origin,
    dependencies,
    hosts,
    identities,
    keys,
    working,
    connected,
    temporaryConnected,
    setNonce: (value: string) => {
      current = value;
    },
  };
}
it("temporary metadata uses the original identity and connect dependency, with save offered only after registration", async () => {
  const f = fixture();
  const target = {
    version: 1 as const,
    token: nonce,
    target: {
      id: hostId,
      label: "临时目标",
      host: "127.0.0.1",
      port: 22,
      username: "user",
    },
  };
  await f.controller.open(hostId, target);
  expect(f.dependencies.hosts).not.toHaveBeenCalled();
  expect(f.temporaryConnected).not.toHaveBeenCalled();
  expect(f.selectedHost.textContent).toContain("临时目标");
  f.connect.dispatchEvent(new Event("click"));
  await vi.waitFor(() =>
    expect(f.temporaryConnected).toHaveBeenCalledWith(nonce, target),
  );
  expect(f.dependencies.connect).toHaveBeenCalledWith(
    nonce,
    { host: hostId, identity: identityId },
    expect.any(AbortSignal),
    expect.any(Function),
  );
  expect(f.connected).toHaveBeenCalledWith(nonce, "1");
  expect(f.temporaryConnected).toHaveBeenCalledWith(nonce, target);
});
it("temporary MFA requires explicit selection and publishes save only after original registration", async () => {
  const f = fixture();
  for (const identity of f.identities.identities)
    identity.secret.hasValue = false;
  const target = {
    version: 1 as const,
    token: nonce,
    target: {
      id: hostId,
      label: "temporary MFA",
      host: "127.0.0.1",
      port: 22,
      username: "user",
    },
  };
  const started = deferred<string>();
  f.dependencies.connectInteractive.mockImplementation(() => started.promise);
  await f.controller.open(hostId, target);
  expect(f.identity.value).toBe("");
  expect(f.connect.disabled).toBe(true);
  f.identity.value = `interactive:${identityId}`;
  f.identity.dispatchEvent(new Event("change"));
  expect(f.connect.disabled).toBe(false);
  f.connect.dispatchEvent(new Event("click"));
  await vi.waitFor(() =>
    expect(f.dependencies.connectInteractive).toHaveBeenCalledWith(
      nonce,
      { host: hostId, identity: identityId },
      expect.any(AbortSignal),
      expect.any(Function),
      expect.any(Function),
    ),
  );
  expect(f.temporaryConnected).not.toHaveBeenCalled();
  started.resolve("9");
  await vi.waitFor(() => expect(f.connected).toHaveBeenCalledWith(nonce, "9"));
  expect(f.temporaryConnected).toHaveBeenCalledWith(nonce, target);
  expect(f.dependencies.connect).not.toHaveBeenCalled();
});
it("temporary MFA rejection does not offer saving or fall back to stored authentication", async () => {
  const f = fixture();
  const target = {
    version: 1 as const,
    token: nonce,
    target: {
      id: hostId,
      label: "temporary MFA",
      host: "127.0.0.1",
      port: 22,
      username: "user",
    },
  };
  f.dependencies.connectInteractive.mockRejectedValue(Error("denied"));
  await f.controller.open(hostId, target);
  f.identity.value = `interactive:${identityId}`;
  f.identity.dispatchEvent(new Event("change"));
  f.connect.dispatchEvent(new Event("click"));
  await vi.waitFor(() =>
    expect(f.dependencies.connectInteractive).toHaveBeenCalled(),
  );
  await vi.waitFor(() => expect(f.status.textContent).toContain("未自动重试"));
  expect(f.connect.disabled).toBe(true);
  expect(f.temporaryConnected).not.toHaveBeenCalled();
  expect(f.connected).not.toHaveBeenCalled();
  expect(f.dependencies.connect).not.toHaveBeenCalled();
});

function keyRows(hasPassphrase = true): KeyIdentityCatalogue["identities"] {
  return [
    {
      id: identityId,
      label: "<b>key</b>",
      username: "user",
      secret: { id: nonce, kind: "ssh_private_key", hasValue: true },
      passphrase: {
        id: "33333333-3333-4333-8333-333333333333",
        kind: "ssh_key_passphrase",
        hasValue: hasPassphrase,
      },
    },
  ];
}
it("same-numbered key and password options dispatch distinct saved identity paths", async () => {
  const f = fixture();
  f.dependencies.keyIdentities.mockResolvedValue({
    ...f.keys,
    identities: keyRows(),
  });
  await f.controller.open();
  expect(f.identity.value).toBe(identityId);
  expect(
    Array.from(f.identity.children).find(
      (option) => option.value === `key:${identityId}`,
    )?.textContent,
  ).toBe("<b>key</b> · user · 私钥");
  f.identity.value = `key:${identityId}`;
  f.connect.dispatchEvent(new Event("click"));
  await vi.waitFor(() => expect(f.connected).toHaveBeenCalledWith(nonce, "2"));
  expect(f.dependencies.connect).not.toHaveBeenCalled();
  expect(f.dependencies.connectKey).toHaveBeenCalledWith(
    nonce,
    { host: hostId, identity: identityId },
    expect.any(AbortSignal),
    expect.any(Function),
  );
});
it("missing passphrase disables the key without choosing a password fallback", async () => {
  const f = fixture();
  f.dependencies.identities.mockResolvedValue({
    ...f.identities,
    identities: [],
  });
  f.dependencies.keyIdentities.mockResolvedValue({
    ...f.keys,
    identities: keyRows(false),
  });
  await f.controller.open();
  expect(f.host.disabled).toBe(false);
  expect(f.connect.disabled).toBe(true);
  f.connect.dispatchEvent(new Event("click"));
  expect(f.dependencies.connectKey).not.toHaveBeenCalled();
  expect(f.dependencies.connect).not.toHaveBeenCalled();
});
it("key-only catalogue chooses its saved identity after three sequential reads", async () => {
  const f = fixture(),
    pending = deferred<typeof f.identities>();
  f.dependencies.identities.mockReturnValue(pending.promise);
  const opened = f.controller.open();
  await vi.waitFor(() => expect(f.dependencies.identities).toHaveBeenCalled());
  expect(f.dependencies.keyIdentities).not.toHaveBeenCalled();
  f.dependencies.keyIdentities.mockResolvedValue({
    ...f.keys,
    identities: keyRows(),
  });
  pending.resolve({ ...f.identities, identities: [] });
  await opened;
  expect(f.identity.value).toBe(`key:${identityId}`);
  expect(f.connect.disabled).toBe(false);
});
it("late key metadata cannot repopulate a closed dialog or move focus", async () => {
  const f = fixture(),
    pending = deferred<KeyIdentityCatalogue>();
  f.dependencies.keyIdentities.mockReturnValue(pending.promise);
  const opened = f.controller.open();
  await vi.waitFor(() =>
    expect(f.dependencies.keyIdentities).toHaveBeenCalled(),
  );
  f.controller.close();
  const focused = f.origin.focuses;
  pending.resolve({ ...f.keys, identities: keyRows() });
  await opened;
  expect(f.identity.children).toEqual([]);
  expect(f.origin.focuses).toBe(focused);
  expect(f.connect.disabled).toBe(true);
});
it("cancelled key connection cannot publish a late owned session", async () => {
  const f = fixture(),
    pending = deferred<string>();
  f.dependencies.identities.mockResolvedValue({
    ...f.identities,
    identities: [],
  });
  f.dependencies.keyIdentities.mockResolvedValue({
    ...f.keys,
    identities: keyRows(),
  });
  f.dependencies.connectKey.mockReturnValue(pending.promise);
  await f.controller.open();
  f.connect.dispatchEvent(new Event("click"));
  const signal = f.dependencies.connectKey.mock.calls[0]?.[2];
  f.controller.close();
  expect(signal?.aborted).toBe(true);
  pending.resolve("2");
  await Promise.resolve();
  await Promise.resolve();
  expect(f.connected).not.toHaveBeenCalled();
  expect(f.dependencies.connect).not.toHaveBeenCalled();
});
it("preselects the requested saved host while keeping approval and connect explicit", async () => {
  const f = fixture();
  const second = "33333333-3333-7333-8333-333333333333";
  f.hosts.hosts.push({
    id: second,
    label: "second",
    host: "fixture.invalid",
    port: 22,
    username: "u",
  });
  await f.controller.open(second);
  expect(f.host.value).toBe(second);
  expect(f.selectedHost.textContent).toBe(
    "当前主机：second · fixture.invalid:22 · 当前身份：fixed · user",
  );
  f.host.value = hostId;
  f.host.dispatchEvent(new Event("change"));
  expect(f.selectedHost.textContent).toBe(
    "当前主机：<img src=x> · 127.0.0.1:22 · 当前身份：fixed · user",
  );
  expect(f.dependencies.connect).not.toHaveBeenCalled();
  expect(f.dependencies.connectKey).not.toHaveBeenCalled();
  f.cancel.dispatchEvent(new Event("click"));
  expect(f.connected).not.toHaveBeenCalled();
  expect(f.selectedHost.textContent).toBe("");
});
it("a removed or foreign tree item never falls back to another saved host", async () => {
  const f = fixture();
  await f.controller.open("33333333-3333-7333-8333-333333333333");
  expect(f.connect.disabled).toBe(true);
  expect(f.host.children).toHaveLength(0);
  expect(f.dependencies.identities).not.toHaveBeenCalled();
  f.connect.dispatchEvent(new Event("click"));
  expect(f.dependencies.connect).not.toHaveBeenCalled();
  expect(f.dependencies.connectKey).not.toHaveBeenCalled();
  f.cancel.dispatchEvent(new Event("click"));
});

it("preselects saved password and key defaults using existing connection paths", async () => {
  for (const kind of ["password", "key"] as const) {
    const f = fixture();
    const keys = keyRows().map((key) => ({
      ...key,
      id: "66666666-6666-7666-8666-666666666666",
    }));
    const selected = kind === "key" ? (keys[0]?.id ?? "") : identityId;
    f.dependencies.hosts.mockResolvedValue({
      ...f.hosts,
      hosts: f.hosts.hosts.map((h) => ({
        ...h,
        username: "user",
        profile: {
          id: "44444444-4444-7444-8444-444444444444",
          label: "Profile",
          identityId: selected,
        },
      })),
    });
    if (kind === "key")
      f.dependencies.keyIdentities.mockResolvedValue({
        ...f.keys,
        vaultPresent: true,
        identities: keys,
      });
    await f.controller.open();
    expect(f.identity.value).toBe(
      kind === "key" ? `key:${selected}` : selected,
    );
    expect(f.connect.disabled).toBe(false);
    expect(f.dependencies.connect).not.toHaveBeenCalled();
    expect(f.dependencies.connectKey).not.toHaveBeenCalled();
    f.controller.close();
  }
});
it("missing, unset, mismatched or ambiguous default never selects another identity", async () => {
  for (const selected of [
    null,
    "44444444-4444-7444-8444-444444444444",
    identityId,
    "ambiguous",
  ]) {
    const f = fixture();
    if (selected === "ambiguous")
      f.dependencies.keyIdentities.mockResolvedValue({
        ...f.keys,
        identities: keyRows(),
      });
    f.dependencies.hosts.mockResolvedValue({
      ...f.hosts,
      hosts: f.hosts.hosts.map((h) => ({
        ...h,
        username:
          selected === "ambiguous"
            ? "user"
            : selected === identityId
              ? "other"
              : "u",
        profile: {
          id: "55555555-5555-7555-8555-555555555555",
          label: "Profile",
          identityId: selected === "ambiguous" ? identityId : selected,
        },
      })),
    });
    await f.controller.open();
    expect(f.identity.value).toBe("");
    expect(f.connect.disabled).toBe(true);
    f.connect.dispatchEvent(new Event("click"));
    expect(f.dependencies.connect).not.toHaveBeenCalled();
    expect(f.dependencies.connectKey).not.toHaveBeenCalled();
    f.controller.close();
  }
});
afterEach(() => vi.unstubAllGlobals());
it("uses literal saved options only and never reads a password", async () => {
  const f = fixture();
  await f.controller.open();
  expect(f.host.children[0]?.textContent).toBe("<img src=x> · 127.0.0.1:22");
  expect(f.connect.disabled).toBe(false);
  f.connect.dispatchEvent(new Event("click"));
  await vi.waitFor(() => expect(f.connected).toHaveBeenCalledWith(nonce, "1"));
  expect(f.dependencies.connect).toHaveBeenCalledWith(
    nonce,
    { host: hostId, identity: identityId },
    expect.any(AbortSignal),
    expect.any(Function),
  );
  expect(f.dialog.open).toBe(false);
});
it("empty or missing Vault never creates a key or connects", async () => {
  const f = fixture();
  f.dependencies.identities.mockResolvedValue({
    ...f.identities,
    vaultPresent: false,
    identities: [],
  });
  await f.controller.open();
  expect(f.connect.disabled).toBe(true);
  f.connect.dispatchEvent(new Event("click"));
  expect(f.dependencies.connect).not.toHaveBeenCalled();
  expect(f.status.textContent).toContain("保存 SSH 身份");
});
it("metadata reads respect one native credit by running sequentially", async () => {
  const f = fixture(),
    pending = deferred<typeof f.hosts>();
  f.dependencies.hosts.mockReturnValue(pending.promise);
  const open = f.controller.open();
  expect(f.dependencies.identities).not.toHaveBeenCalled();
  pending.resolve(f.hosts);
  await open;
  expect(f.dependencies.identities).toHaveBeenCalledTimes(1);
});
it("cancelled metadata cannot draw or refocus after close", async () => {
  const f = fixture(),
    pending = deferred<typeof f.hosts>();
  f.dependencies.hosts.mockReturnValue(pending.promise);
  const open = f.controller.open();
  f.cancel.dispatchEvent(new Event("click"));
  const focuses = f.origin.focuses;
  pending.resolve(f.hosts);
  await open;
  expect(f.host.children).toEqual([]);
  expect(f.dependencies.identities).not.toHaveBeenCalled();
  expect(f.origin.focuses).toBe(focuses);
});
it("old nonce results cannot enable connect or steal focus", async () => {
  const f = fixture(),
    pending = deferred<typeof f.identities>();
  f.dependencies.identities.mockReturnValue(pending.promise);
  const open = f.controller.open();
  await vi.waitFor(() => expect(f.dependencies.identities).toHaveBeenCalled());
  const before = f.host.focuses;
  f.setNonce("33333333-3333-4333-8333-333333333333");
  pending.resolve(f.identities);
  await open;
  expect(f.connect.disabled).toBe(true);
  expect(f.host.focuses).toBe(before);
  expect(f.connected).not.toHaveBeenCalled();
});
it("cancel pending start revokes it without publishing a late root session", async () => {
  const f = fixture(),
    pending = deferred<string>();
  f.dependencies.connect.mockReturnValue(pending.promise);
  await f.controller.open();
  f.connect.dispatchEvent(new Event("click"));
  const signal = f.dependencies.connect.mock.calls[0]?.[2];
  f.cancel.dispatchEvent(new Event("click"));
  expect(signal?.aborted).toBe(true);
  pending.resolve("1");
  await Promise.resolve();
  await Promise.resolve();
  expect(f.connected).not.toHaveBeenCalled();
  expect(f.dialog.open).toBe(false);
});

const routeProfile = "33333333-3333-7333-8333-333333333333",
  routeVia = "44444444-4444-7444-8444-444444444444";
function routeFixture() {
  const f = fixture();
  const routeHosts = {
    ...f.hosts,
    hosts: [
      {
        ...f.hosts.hosts[0]!,
        username: "user",
        profile: {
          id: routeProfile,
          label: "Leaf",
          identityId: identityId,
          via: routeVia,
        },
      },
    ],
  };
  f.dependencies.hosts.mockResolvedValue(routeHosts);
  return f;
}
const routePrompt: RoutePrompt = {
  index: 0,
  count: 2,
  entrance: hostId,
  label: "Jump",
  host: "127.0.0.1",
  port: 22,
  username: "user",
  identity: identityId,
  identityLabel: "GUI identity",
  algorithm: "ssh-ed25519",
  fingerprint: "SHA256:checked-peer",
  firstUse: true,
};
it("route selection stays explicit and each peer requires its own focused button action", async () => {
  const f = routeFixture();
  f.dependencies.connectRoute.mockImplementation(
    async (_nonce, selection, _signal, _current, confirm) => {
      expect(selection.profile).toBe(routeProfile);
      expect(selection.host).toBe(hostId);
      expect(await confirm(routePrompt)).toBe(true);
      expect(
        await confirm({
          ...routePrompt,
          index: 1,
          label: "Leaf",
          firstUse: false,
        }),
      ).toBe(true);
      return "3";
    },
  );
  await f.controller.open();
  f.connect.dispatchEvent(new Event("click"));
  await vi.waitFor(() => expect(f.connect.textContent).toBe("信任并连接此跳"));
  expect(f.connected).not.toHaveBeenCalled();
  expect(f.connect.disabled).toBe(false);
  expect(f.cancel.disabled).toBe(false);
  expect(f.status.textContent).toContain("SHA256:checked-peer");
  expect(f.connect.focuses).toBeGreaterThan(0);
  f.connect.dispatchEvent(new Event("click"));
  await vi.waitFor(() => expect(f.connect.textContent).toBe("确认连接此跳"));
  expect(f.connected).not.toHaveBeenCalled();
  f.connect.dispatchEvent(new Event("click"));
  await vi.waitFor(() => expect(f.connected).toHaveBeenCalledWith(nonce, "3"));
  expect(f.dependencies.connect).not.toHaveBeenCalled();
});
it("canceling a visible route prompt aborts the original request without publishing a session", async () => {
  const f = routeFixture();
  let answer: RouteDecision | undefined;
  f.dependencies.connectRoute.mockImplementation(
    async (_nonce, _selection, signal, _current, confirm) => {
      answer = await confirm(routePrompt);
      expect(signal.aborted).toBe(true);
      return "3";
    },
  );
  await f.controller.open();
  f.connect.dispatchEvent(new Event("click"));
  await vi.waitFor(() => expect(f.connect.textContent).toBe("信任并连接此跳"));
  f.cancel.dispatchEvent(new Event("click"));
  await vi.waitFor(() => expect(answer).toBe(false));
  expect(f.connected).not.toHaveBeenCalled();
  expect(f.dialog.open).toBe(false);
  expect(f.connect.textContent).toBe("连接");
});

it("saved Agent default chooses the Agent branch and only passes canonical saved IDs", async () => {
  const f = fixture();
  const host = f.hosts.hosts[0];
  if (!host) throw Error("fixture.host");
  Object.assign(host, { defaultIdentity: identityId, username: "agent-user" });
  f.identities.identities.splice(0);
  Object.assign(f.keys, { identities: [] });
  f.dependencies.agentIdentities.mockResolvedValue({
    version: 1,
    token: nonce,
    revision: "1",
    keys: [],
    identities: [
      {
        id: identityId,
        label: "Agent",
        username: "agent-user",
        algorithm: "ssh-ed25519",
        fingerprint: `SHA256:${"A".repeat(43)}`,
      },
    ],
  });
  await f.controller.open();
  expect(f.identity.value).toBe(`agent:${identityId}`);
  f.connect.dispatchEvent(new Event("click"));
  await vi.waitFor(() => expect(f.connected).toHaveBeenCalledWith(nonce, "8"));
  expect(f.dependencies.connectAgent).toHaveBeenCalledWith(
    nonce,
    { host: hostId, identity: identityId },
    expect.any(AbortSignal),
    expect.any(Function),
  );
  expect(f.dependencies.connect).not.toHaveBeenCalled();
  expect(f.dependencies.connectKey).not.toHaveBeenCalled();
});
it("explicit saved password plus MFA uses only the joint connector", async () => {
  const f = fixture();
  await f.controller.open();
  f.identity.value = `password-mfa:${identityId}`;
  f.connect.dispatchEvent(new Event("click"));
  await vi.waitFor(() => expect(f.connected).toHaveBeenCalledWith(nonce, "12"));
  expect(f.dependencies.connectPasswordInteractive).toHaveBeenCalledWith(
    nonce,
    { host: hostId, identity: identityId },
    expect.any(AbortSignal),
    expect.any(Function),
    expect.any(Function),
  );
  expect(f.dependencies.connect).not.toHaveBeenCalled();
  expect(f.dependencies.connectInteractive).not.toHaveBeenCalled();
  expect(f.dependencies.connectKey).not.toHaveBeenCalled();
  expect(f.dependencies.connectRoute).not.toHaveBeenCalled();
});
it("password joint selection is unavailable on routes and never falls back to ordinary password", async () => {
  const f = routeFixture();
  await f.controller.open();
  const option = f.identity.options.find(
    (o) => o.value === `password-mfa:${identityId}`,
  );
  expect(option?.disabled).toBe(true);
  f.identity.value = `password-mfa:${identityId}`;
  f.connect.dispatchEvent(new Event("click"));
  await vi.waitFor(() => expect(f.status.textContent).toContain("连接未成功"));
  expect(f.dependencies.connectPasswordInteractive).not.toHaveBeenCalled();
  expect(f.dependencies.connect).not.toHaveBeenCalled();
  expect(f.dependencies.connectRoute).not.toHaveBeenCalled();
});
it("explicit private key plus MFA uses only its own connector and does not alter saved identities", async () => {
  const f = fixture();
  f.dependencies.keyIdentities.mockResolvedValue({
    ...f.keys,
    identities: keyRows(),
  });
  await f.controller.open();
  f.identity.value = `key-mfa:${identityId}`;
  f.connect.dispatchEvent(new Event("click"));
  await vi.waitFor(() => expect(f.connected).toHaveBeenCalledWith(nonce, "10"));
  expect(f.dependencies.connectKeyInteractive).toHaveBeenCalledWith(
    nonce,
    { host: hostId, identity: identityId },
    expect.any(AbortSignal),
    expect.any(Function),
    expect.any(Function),
  );
  expect(f.dependencies.connectKey).not.toHaveBeenCalled();
  expect(f.dependencies.connectInteractive).not.toHaveBeenCalled();
  expect(f.dependencies.connectRoute).not.toHaveBeenCalled();
});
it("a joint leaf selection preserves the saved route and uses no direct startup", async () => {
  const f = routeFixture();
  f.dependencies.keyIdentities.mockResolvedValue({
    ...f.keys,
    identities: keyRows(),
  });
  f.identities.identities.splice(0);
  await f.controller.open();
  f.identity.value = `key-mfa:${identityId}`;
  f.connect.dispatchEvent(new Event("click"));
  await vi.waitFor(() =>
    expect(f.dependencies.connectRoute).toHaveBeenCalled(),
  );
  expect(f.dependencies.connectRoute.mock.calls[0]?.[1]).toMatchObject({
    host: hostId,
    identity: identityId,
    privateKey: true,
  });
  expect(f.dependencies.connectKeyInteractive).not.toHaveBeenCalled();
  expect(f.dependencies.connectKey).not.toHaveBeenCalled();
});

it("Agent leaf with a saved route uses the route driver and never either direct startup", async () => {
  const f = routeFixture();
  f.identities.identities.splice(0);
  Object.assign(f.keys, { identities: [] });
  f.dependencies.agentIdentities.mockResolvedValue({
    version: 1,
    token: nonce,
    revision: "1",
    keys: [],
    identities: [
      {
        id: identityId,
        label: "Route Agent",
        username: "user",
        algorithm: "ssh-ed25519",
        fingerprint: `SHA256:${"A".repeat(43)}`,
      },
    ],
  });
  await f.controller.open();
  expect(f.identity.value).toBe(`agent:${identityId}`);
  f.connect.dispatchEvent(new Event("click"));
  await vi.waitFor(() =>
    expect(f.dependencies.connectRoute).toHaveBeenCalled(),
  );
  expect(f.dependencies.connectRoute.mock.calls[0]?.[1]).toEqual({
    host: hostId,
    identity: identityId,
    profile: routeProfile,
    privateKey: false,
    agent: true,
  });
  expect(f.dependencies.connectAgent).not.toHaveBeenCalled();
  expect(f.dependencies.connect).not.toHaveBeenCalled();
  expect(f.dependencies.connectKey).not.toHaveBeenCalled();
});

it("CLI route consumes inherited token and closes modal before Root selection without a GUI begin", async () => {
  const f = fixture();
  const pending = f.controller.continueCliRoute(nonce, nonce);
  expect(f.dialog.open).toBe(true);
  await pending;
  expect(f.dependencies.continueRoute).toHaveBeenCalledWith(
    nonce,
    nonce,
    expect.any(AbortSignal),
    expect.any(Function),
    expect.any(Function),
  );
  expect(f.dialog.open).toBe(false);
  expect(f.dependencies.connectRoute).not.toHaveBeenCalled();
  expect(f.connected).not.toHaveBeenCalled();
  expect(f.working).toHaveBeenLastCalledWith(false);
});
it("CLI route cancellation revokes its exact signal and late registration cannot reopen the modal", async () => {
  const f = fixture();
  const held = deferred<string>();
  f.dependencies.continueRoute.mockReturnValue(held.promise);
  const pending = f.controller.continueCliRoute(nonce, nonce);
  await expect(f.controller.continueCliRoute(nonce, nonce)).rejects.toThrow(
    "ssh.route_denied",
  );
  const signal = f.dependencies.continueRoute.mock.calls[0]![2];
  f.cancel.dispatchEvent(new Event("click"));
  expect(signal.aborted).toBe(true);
  held.resolve("3");
  await pending;
  expect(f.dialog.open).toBe(false);
  expect(f.connected).not.toHaveBeenCalled();
  expect(f.dependencies.connectRoute).not.toHaveBeenCalled();
});

it("metadata-only accounts require explicit MFA selection and never call a saved credential path", async () => {
  const f = fixture();
  f.identities.identities[0]!.secret.hasValue = false;
  await f.controller.open();
  expect(f.identity.disabled).toBe(false);
  expect(f.identity.value).toBe("");
  expect(f.connect.disabled).toBe(true);
  expect(f.dependencies.connectInteractive).not.toHaveBeenCalled();
  f.identity.value = `interactive:${identityId}`;
  f.identity.dispatchEvent(new Event("change"));
  expect(f.connect.disabled).toBe(false);
  f.connect.dispatchEvent(new Event("click"));
  await vi.waitFor(() => expect(f.connected).toHaveBeenCalledWith(nonce, "9"));
  expect(f.dependencies.connectInteractive).toHaveBeenCalledWith(
    nonce,
    { host: hostId, identity: identityId },
    expect.any(AbortSignal),
    expect.any(Function),
    expect.any(Function),
  );
  expect(f.dependencies.connect).not.toHaveBeenCalled();
  expect(f.dependencies.connectKey).not.toHaveBeenCalled();
  expect(f.dependencies.connectAgent).not.toHaveBeenCalled();
});

function agentJointFixture(route = false) {
  const f = route ? routeFixture() : fixture();
  Object.assign(f.hosts.hosts[0]!, { username: "user" });
  f.identities.identities.splice(0);
  Object.assign(f.keys, { identities: [] });
  f.dependencies.agentIdentities.mockResolvedValue({
    version: 1,
    token: nonce,
    revision: "1",
    keys: [],
    identities: [
      {
        id: identityId,
        label: "Selected Agent",
        username: "user",
        algorithm: "ssh-ed25519",
        fingerprint: `SHA256:${"A".repeat(43)}`,
      },
    ],
  });
  return f;
}
it("Agent plus MFA is explicit and submits only the saved identity to its own flow", async () => {
  const f = agentJointFixture();
  await f.controller.open();
  expect(f.identity.value).toBe(`agent:${identityId}`);
  f.identity.value = `agent-joint:${identityId}`;
  f.identity.dispatchEvent(new Event("change"));
  f.connect.dispatchEvent(new Event("click"));
  await vi.waitFor(() => expect(f.connected).toHaveBeenCalledWith(nonce, "11"));
  expect(
    f.dependencies.connectAgentInteractive.mock.calls[0]?.slice(0, 2),
  ).toEqual([nonce, { host: hostId, identity: identityId }]);
  expect(f.dependencies.connectAgent).not.toHaveBeenCalled();
  expect(f.dependencies.connectKeyInteractive).not.toHaveBeenCalled();
  expect(f.dependencies.connectInteractive).not.toHaveBeenCalled();
  expect(f.dependencies.connect).not.toHaveBeenCalled();
});
it("Agent plus MFA uses the exact saved route and cannot bypass it through a direct flow", async () => {
  const f = agentJointFixture(true);
  await f.controller.open();
  expect(
    Array.from(f.identity.options).find(
      (o) => o.value === `agent-joint:${identityId}`,
    )?.disabled,
  ).toBe(false);
  f.identity.value = `agent-joint:${identityId}`;
  f.identity.dispatchEvent(new Event("change"));
  f.connect.dispatchEvent(new Event("click"));
  await vi.waitFor(() => expect(f.connected).toHaveBeenCalledWith(nonce, "3"));
  expect(f.dependencies.connectRoute).toHaveBeenCalledWith(
    nonce,
    expect.objectContaining({
      host: hostId,
      identity: identityId,
      privateKey: false,
      agent: true,
    }),
    expect.any(AbortSignal),
    expect.any(Function),
    expect.any(Function),
  );
  expect(f.dependencies.connectAgentInteractive).not.toHaveBeenCalled();
  expect(f.dependencies.connectAgent).not.toHaveBeenCalled();
  expect(f.dependencies.connectKeyInteractive).not.toHaveBeenCalled();
  expect(f.dependencies.connect).not.toHaveBeenCalled();
});

it("temporary Agent plus MFA stays explicit, publishes save only after registration and never falls back", async () => {
  for (const reject of [false, true]) {
    const f = agentJointFixture();
    const target = {
      version: 1 as const,
      token: nonce,
      target: {
        id: hostId,
        label: "Temporary Agent MFA",
        host: "127.0.0.1",
        port: 22,
        username: "user",
      },
    };
    const pending = deferred<string>();
    f.dependencies.connectAgentInteractive.mockImplementation(
      () => pending.promise,
    );
    await f.controller.open(hostId, target);
    expect(f.identity.value).toBe(`agent:${identityId}`);
    expect(
      f.identity.options.find(
        (option) => option.value === `agent-joint:${identityId}`,
      )?.disabled,
    ).toBe(false);
    f.identity.value = `agent-joint:${identityId}`;
    f.identity.dispatchEvent(new Event("change"));
    f.connect.dispatchEvent(new Event("click"));
    await vi.waitFor(() =>
      expect(f.dependencies.connectAgentInteractive).toHaveBeenCalledTimes(1),
    );
    expect(f.temporaryConnected).not.toHaveBeenCalled();
    expect(
      f.dependencies.connectAgentInteractive.mock.calls[0]?.slice(0, 2),
    ).toEqual([nonce, { host: hostId, identity: identityId }]);
    if (reject) f.controller.close();
    pending.resolve("11");
    if (reject) {
      await new Promise((resolve) => setTimeout(resolve, 0));
      expect(f.connected).not.toHaveBeenCalled();
      expect(f.temporaryConnected).not.toHaveBeenCalled();
    } else {
      await vi.waitFor(() =>
        expect(f.temporaryConnected).toHaveBeenCalledWith(nonce, target),
      );
      expect(f.connected).toHaveBeenCalledWith(nonce, "11");
    }
    expect(f.dependencies.connectAgent).not.toHaveBeenCalled();
    expect(f.dependencies.connect).not.toHaveBeenCalled();
    expect(f.dependencies.connectKeyInteractive).not.toHaveBeenCalled();
    expect(f.dependencies.connectInteractive).not.toHaveBeenCalled();
  }
});

it("temporary password plus MFA stays explicit and publishes saving only after current registration", async () => {
  for (const outcome of ["success", "failure", "cancel"] as const) {
    const f = fixture();
    const target = {
      version: 1 as const,
      token: nonce,
      target: {
        id: hostId,
        label: "Temporary password MFA",
        host: "127.0.0.1",
        port: 22,
        username: "user",
      },
    };
    const pending = deferred<string>();
    f.dependencies.connectPasswordInteractive.mockImplementation(
      () => pending.promise,
    );
    await f.controller.open(hostId, target);
    expect(f.identity.value).toBe(identityId);
    expect(
      f.identity.options.find(
        (option) => option.value === `password-mfa:${identityId}`,
      )?.disabled,
    ).toBe(false);
    f.identity.value = `password-mfa:${identityId}`;
    f.identity.dispatchEvent(new Event("change"));
    f.connect.dispatchEvent(new Event("click"));
    await vi.waitFor(() =>
      expect(f.dependencies.connectPasswordInteractive).toHaveBeenCalledTimes(
        1,
      ),
    );
    expect(
      f.dependencies.connectPasswordInteractive.mock.calls[0]?.slice(0, 2),
    ).toEqual([nonce, { host: hostId, identity: identityId }]);
    expect(f.temporaryConnected).not.toHaveBeenCalled();
    if (outcome === "cancel") f.controller.close();
    if (outcome === "failure") pending.reject(Error("ssh.session_denied"));
    else pending.resolve("12");
    if (outcome === "success") {
      await vi.waitFor(() =>
        expect(f.temporaryConnected).toHaveBeenCalledWith(nonce, target),
      );
      expect(f.connected).toHaveBeenCalledWith(nonce, "12");
    } else {
      if (outcome === "failure")
        await vi.waitFor(() =>
          expect(f.status.textContent).toContain("连接未成功"),
        );
      else await new Promise((resolve) => setTimeout(resolve, 0));
      expect(f.connected).not.toHaveBeenCalled();
      expect(f.temporaryConnected).not.toHaveBeenCalled();
    }
    expect(f.dependencies.connect).not.toHaveBeenCalled();
    expect(f.dependencies.connectInteractive).not.toHaveBeenCalled();
    expect(f.dependencies.connectKeyInteractive).not.toHaveBeenCalled();
    expect(f.dependencies.connectAgentInteractive).not.toHaveBeenCalled();
    expect(f.dependencies.connectRoute).not.toHaveBeenCalled();
  }
});
