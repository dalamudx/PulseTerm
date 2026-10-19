import {
  type AgentCatalogue,
  cancelAgentCatalogue,
  readAgentCatalogue,
} from "./workspace-agent-identities";
import { type Host, type HostCatalogue, readHosts } from "./workspace-hosts";
import { type IdentityCatalogue, readIdentities } from "./workspace-identities";
import {
  type KeyIdentityCatalogue,
  readKeyIdentities,
} from "./workspace-key-identities";
import { message, uiAttribute, uiContent, uiText } from "./workspace-language";
import { answerInteractivePrompt } from "./workspace-ssh-interactive";
import {
  connectSshRoute,
  continueSshRoute,
  type RouteDecision,
  type RoutePrompt,
} from "./workspace-ssh-route";
import {
  connectSavedAgentInteractiveSsh,
  connectSavedAgentSsh,
  connectSavedInteractiveSsh,
  connectSavedPasswordInteractiveSsh,
  connectSavedPrivateKeyInteractiveSsh,
  connectSavedPrivateKeySsh,
  connectSavedSsh,
  type SavedSshSelection,
} from "./workspace-ssh-session";
import {
  cancelTemporaryTarget,
  type TemporaryTarget,
} from "./workspace-temporary-target";
export class WorkspaceSshDialog {
  private routeAnswer: (() => void) | undefined;
  private temporary: TemporaryTarget | undefined;
  private epoch = 0;
  private disposed = false;
  private busy = false;
  private controller: AbortController | undefined;
  private originalNonce: string | undefined;
  private focus: HTMLElement | undefined;
  private readonly agentChoices = new Map<string, string>();
  private readonly agentJointChoices = new Map<string, string>();
  private readonly interactiveChoices = new Map<string, string>();
  private readonly jointChoices = new Map<string, string>();
  private readonly passwordJointChoices = new Map<string, string>();
  private readonly keyChoices = new Map<string, string>();
  private readonly choiceUsers = new Map<string, string>();
  private hosts: readonly Host[] = [];
  private ready = false;
  constructor(
    private readonly options: {
      dialog: HTMLDialogElement;
      host: HTMLSelectElement;
      identity: HTMLSelectElement;
      connect: HTMLButtonElement;
      fingerprint: HTMLButtonElement;
      fingerprintEnabled(): boolean;
      inspectFingerprint(host: string): void;
      cancel: HTMLButtonElement;
      status: HTMLElement;
      selectedHost: HTMLElement;
      signal: AbortSignal;
      nonce(): string | undefined;
      working(busy: boolean): void;
      connected(nonce: string, id: string): Promise<void>;
      temporaryConnected?(nonce: string, target: TemporaryTarget): void;
    },
    private readonly dependencies: {
      hosts(nonce: string, signal: AbortSignal): Promise<HostCatalogue>;
      identities(
        nonce: string,
        signal: AbortSignal,
      ): Promise<IdentityCatalogue>;
      keyIdentities?(
        nonce: string,
        signal: AbortSignal,
      ): Promise<KeyIdentityCatalogue>;
      agentIdentities?(
        nonce: string,
        signal: AbortSignal,
      ): Promise<AgentCatalogue>;
      connectAgent?: typeof connectSavedAgentSsh;
      connectAgentInteractive?: typeof connectSavedAgentInteractiveSsh;
      connectInteractive?: typeof connectSavedInteractiveSsh;
      connectKeyInteractive?: typeof connectSavedPrivateKeyInteractiveSsh;
      connectPasswordInteractive?: typeof connectSavedPasswordInteractiveSsh;
      connectRoute?: typeof connectSshRoute;
      continueRoute?: typeof continueSshRoute;
      connectKey?(
        nonce: string,
        selection: SavedSshSelection,
        signal: AbortSignal,
        current: () => boolean,
      ): Promise<string>;
      connect(
        nonce: string,
        selection: SavedSshSelection,
        signal: AbortSignal,
        current: () => boolean,
      ): Promise<string>;
    } = {
      hosts: readHosts,
      identities: readIdentities,
      keyIdentities: readKeyIdentities,
      connectKey: connectSavedPrivateKeySsh,
      agentIdentities: readAgentCatalogue,
      connectAgent: connectSavedAgentSsh,
      connectInteractive: connectSavedInteractiveSsh,
      connectKeyInteractive: connectSavedPrivateKeyInteractiveSsh,
      connectPasswordInteractive: connectSavedPasswordInteractiveSsh,
      connectRoute: connectSshRoute,
      connect: connectSavedSsh,
    },
  ) {
    options.dialog.addEventListener("cancel", (event) => {
      event.preventDefault();
      this.close();
    });
    options.cancel.addEventListener("click", () => this.close());
    options.host.addEventListener("change", () => {
      if (this.current(this.epoch)) {
        this.applyProfile();
        this.describeHost();
      }
    });
    options.identity.addEventListener("change", () => {
      if (this.current(this.epoch)) {
        options.connect.disabled =
          this.busy ||
          !this.ready ||
          !options.identity.value ||
          options.identity.selectedOptions[0]?.disabled !== false;
        this.describeHost();
      }
    });
    options.fingerprint.addEventListener("click", () => {
      if (this.canInspectFingerprint())
        options.inspectFingerprint(options.host.value);
    });
    options.connect.addEventListener("click", () => {
      void this.connect();
    });
    options.signal.addEventListener(
      "abort",
      () => {
        this.disposed = true;
        this.close();
      },
      { once: true },
    );
  }
  private current(epoch: number): boolean {
    return (
      epoch === this.epoch &&
      !this.disposed &&
      !this.options.signal.aborted &&
      !this.controller?.signal.aborted &&
      this.originalNonce === this.options.nonce() &&
      this.options.dialog.open
    );
  }
  private canInspectFingerprint(): boolean {
    const host = this.hosts.find((host) => host.id === this.options.host.value);
    return (
      this.current(this.epoch) &&
      !this.busy &&
      this.options.fingerprintEnabled() &&
      !!host &&
      !host.profile?.via
    );
  }
  private describeHost(): void {
    this.options.fingerprint.disabled = !this.canInspectFingerprint();
    const option = Array.from(this.options.host.options).find(
      (option) => option.value === this.options.host.value,
    );
    const identity = this.options.identity.selectedOptions[0];
    uiText(
      this.options.selectedHost,
      option
        ? message(
            "当前主机：{0} · 当前身份：{1}",
            option.textContent ?? "",
            identity ? uiContent(identity) : message("尚未选择"),
          )
        : "",
    );
  }
  private applyProfile(): void {
    const host = this.hosts.find((h) => h.id === this.options.host.value),
      profile = host?.profile;
    if (!profile) return;
    const id = profile.identityId;
    const choices = id
      ? [id, `key:${id}`, `agent:${id}`].filter((value) =>
          this.choiceUsers.has(value),
        )
      : [];
    const choice = choices.length === 1 ? choices[0] : undefined;
    const option = Array.from(this.options.identity.options).find(
      (o) => o.value === choice,
    );
    if (
      choice &&
      option &&
      !option.disabled &&
      this.choiceUsers.get(choice) === host.username
    ) {
      this.options.identity.value = choice;
      this.options.connect.disabled = !this.ready;
    } else {
      this.options.identity.selectedIndex = -1;
      this.options.connect.disabled = true;
      uiText(
        this.options.status,
        id ? "默认身份不可用或用户名不匹配，请重新选择。" : "请选择连接身份。",
      );
    }
  }
  async open(hostId?: string, temporary?: TemporaryTarget): Promise<void> {
    if (
      this.disposed ||
      this.options.signal.aborted ||
      this.options.dialog.open
    )
      return;
    const nonce = this.options.nonce();
    if (!nonce) return;
    this.originalNonce = nonce;
    this.temporary = temporary;
    uiAttribute(
      this.options.host,
      "aria-label",
      temporary ? "确认临时SSH目标" : "选择保存的SSH主机",
    );
    if (temporary) hostId = temporary.target.id;
    const epoch = ++this.epoch;
    const controller = new AbortController();
    this.controller = controller;
    const focused = this.options.dialog.ownerDocument.activeElement;
    this.focus = focused instanceof HTMLElement ? focused : undefined;
    this.jointChoices.clear();
    this.passwordJointChoices.clear();
    this.keyChoices.clear();
    this.agentChoices.clear();
    this.agentJointChoices.clear();
    this.interactiveChoices.clear();
    this.choiceUsers.clear();
    this.hosts = [];
    this.ready = false;
    this.options.host.replaceChildren();
    this.options.selectedHost.textContent = "";
    this.options.identity.replaceChildren();
    this.options.connect.disabled = true;
    this.options.fingerprint.disabled = true;
    this.options.host.disabled = true;
    this.options.identity.disabled = true;
    this.busy = true;
    this.options.working(true);
    uiText(this.options.status, "正在读取保存的主机与身份…");
    this.options.dialog.showModal();
    this.options.cancel.focus();
    try {
      // The backend has one shared native read credit. Do not race these independent reads.
      const hosts: HostCatalogue = temporary
        ? {
            version: 1,
            token: temporary.token,
            revision: "0",
            hosts: [temporary.target],
          }
        : await this.dependencies.hosts(nonce, controller.signal);
      if (!this.current(epoch)) return;
      if (
        hostId !== undefined &&
        !hosts.hosts.some((host) => host.id === hostId)
      ) {
        uiText(
          this.options.status,
          "所选主机已不在保存目录中，请关闭后重新读取。",
        );
        return;
      }
      const identities = await this.dependencies.identities(
        nonce,
        controller.signal,
      );
      if (!this.current(epoch)) return;
      const keys = await this.dependencies.keyIdentities?.(
        nonce,
        controller.signal,
      );
      if (!this.current(epoch)) return;
      const agents = await this.dependencies.agentIdentities?.(
        nonce,
        controller.signal,
      );
      if (!this.current(epoch)) return;
      if (agents && this.dependencies.agentIdentities === readAgentCatalogue)
        void cancelAgentCatalogue(nonce, agents.token);
      this.hosts = hosts.hosts;
      const doc = this.options.dialog.ownerDocument;
      for (const host of hosts.hosts) {
        const option = doc.createElement("option");
        option.value = host.id;
        option.textContent = `${host.label} · ${host.host}:${host.port}`;
        this.options.host.append(option);
      }
      if (hostId !== undefined) this.options.host.value = hostId;
      this.describeHost();
      for (const identity of identities.identities) {
        this.choiceUsers.set(identity.id, identity.username);
        const option = doc.createElement("option");
        option.value = identity.id;
        uiText(
          option,
          message(
            "{0} · {1}{2}",
            identity.label,
            identity.username,
            identity.secret.hasValue ? "" : message(" · 未保存密码"),
          ),
        );
        option.disabled = !identity.secret.hasValue;
        this.options.identity.append(option);
        if (identity.secret.hasValue) {
          const joint = doc.createElement("option");
          joint.value = `password-mfa:${identity.id}`;
          uiText(
            joint,
            message(
              "{0} · {1} · 密码 + MFA",
              identity.label,
              identity.username,
            ),
          );
          this.passwordJointChoices.set(joint.value, identity.id);
          this.choiceUsers.set(joint.value, identity.username);
          this.options.identity.append(joint);
        }
        const interactive = doc.createElement("option");
        interactive.value = `interactive:${identity.id}`;
        uiText(
          interactive,
          message(
            "{0} · {1} · 交互认证 / MFA",
            identity.label,
            identity.username,
          ),
        );
        this.interactiveChoices.set(interactive.value, identity.id);
        this.choiceUsers.set(interactive.value, identity.username);
        this.options.identity.append(interactive);
      }
      let firstKey: string | undefined;
      for (const identity of keys?.identities ?? []) {
        const option = doc.createElement("option");
        const eligible =
          identity.secret.hasValue &&
          (!identity.passphrase || identity.passphrase.hasValue);
        option.value = `key:${identity.id}`;
        this.choiceUsers.set(option.value, identity.username);
        uiText(
          option,
          message(
            "{0} · {1} · 私钥{2}",
            identity.label,
            identity.username,
            eligible ? "" : message(" · 未保存凭据"),
          ),
        );
        option.disabled = !eligible;
        this.options.identity.append(option);
        if (eligible) {
          this.keyChoices.set(option.value, identity.id);
          firstKey ??= option.value;
          const joint = doc.createElement("option");
          joint.value = `key-mfa:${identity.id}`;
          uiText(
            joint,
            message(
              "{0} · {1} · 私钥 + MFA",
              identity.label,
              identity.username,
            ),
          );
          this.jointChoices.set(joint.value, identity.id);
          this.choiceUsers.set(joint.value, identity.username);
          this.options.identity.append(joint);
        }
      }
      let firstAgent: string | undefined;
      for (const identity of agents?.identities ?? []) {
        const option = doc.createElement("option");
        option.value = `agent:${identity.id}`;
        uiText(
          option,
          message("{0} · {1} · Agent", identity.label, identity.username),
        );
        this.choiceUsers.set(option.value, identity.username);
        this.agentChoices.set(option.value, identity.id);
        this.options.identity.append(option);
        firstAgent ??= option.value;
        const joint = doc.createElement("option");
        joint.value = `agent-joint:${identity.id}`;
        uiText(
          joint,
          message("{0} · {1} · Agent + MFA", identity.label, identity.username),
        );
        this.choiceUsers.set(joint.value, identity.username);
        this.agentJointChoices.set(joint.value, identity.id);
        this.options.identity.append(joint);
      }
      const first = identities.identities.find(
        (identity) => identity.secret.hasValue,
      );
      if (first) this.options.identity.value = first.id;
      if (!first && firstKey) this.options.identity.value = firstKey;
      if (!first && !firstKey && firstAgent)
        this.options.identity.value = firstAgent;
      if (!first && !firstKey && !firstAgent)
        this.options.identity.selectedIndex = -1;
      const ready =
        hosts.hosts.length > 0 &&
        (identities.vaultPresent || keys?.vaultPresent === true) &&
        (first !== undefined ||
          firstKey !== undefined ||
          firstAgent !== undefined ||
          this.interactiveChoices.size > 0);
      this.ready = ready;
      this.options.host.disabled = hosts.hosts.length === 0;
      this.options.identity.disabled = !ready;
      this.options.connect.disabled =
        !ready || (!first && !firstKey && !firstAgent);
      uiText(
        this.options.status,
        ready
          ? "连接前请解锁保险库；连接将使用所选身份和已批准的主机指纹。"
          : hosts.hosts.length === 0
            ? "请先添加主机和保存的 SSH 身份。"
            : "可以选择保存的主机；请先保存 SSH 身份。",
      );
      this.applyProfile();
      this.describeHost();
      if (ready) this.options.host.focus();
    } catch {
      if (this.current(epoch))
        uiText(this.options.status, "无法读取保存的选择，请关闭后重试。");
    } finally {
      if (this.current(epoch)) {
        this.busy = false;
        this.describeHost();
        this.options.working(false);
      }
    }
  }
  /** Consumes only the backend CLI route token; never recaptures a click or starts a new route. */
  async continueCliRoute(nonce: string, token: string): Promise<void> {
    if (
      this.disposed ||
      this.options.signal.aborted ||
      this.options.nonce() !== nonce ||
      this.options.dialog.open
    )
      throw Error("ssh.route_denied");
    this.originalNonce = nonce;
    const epoch = ++this.epoch;
    const controller = new AbortController();
    this.controller = controller;
    this.busy = true;
    this.options.working(true);
    this.options.host.disabled = true;
    this.options.identity.disabled = true;
    this.options.connect.disabled = true;
    this.options.fingerprint.disabled = true;
    this.options.cancel.disabled = false;
    uiText(this.options.status, "正在读取 CLI 已批准路线的实际指纹…");
    this.options.dialog.showModal();
    try {
      await (this.dependencies.continueRoute ?? continueSshRoute)(
        nonce,
        token,
        controller.signal,
        () => this.current(epoch),
        (prompt) => this.confirmRoute(prompt, controller.signal, epoch),
      );
    } finally {
      if (this.current(epoch)) this.close(true);
    }
  }

  private async confirmRoute(
    prompt: RoutePrompt,
    signal: AbortSignal,
    epoch: number,
  ): Promise<RouteDecision> {
    if (!this.current(epoch) || signal.aborted) return false;
    const description = message(
      "第 {0}/{1} 跳：{2}，{3}@{4}:{5}。身份 {6}。{7} {8}。{9}",
      prompt.index + 1,
      prompt.count,
      prompt.label,
      prompt.username,
      prompt.host,
      prompt.port,
      prompt.identityLabel,
      prompt.algorithm,
      prompt.fingerprint,
      message(
        prompt.firstUse
          ? "首次指纹，请核对后信任并连接。"
          : "实际指纹匹配保存记录，确认使用该身份。",
      ),
    );
    uiText(this.options.status, description);
    uiText(this.options.selectedHost, description);
    uiText(
      this.options.connect,
      prompt.firstUse ? "信任并连接此跳" : "确认连接此跳",
    );
    let method: HTMLSelectElement | undefined;
    const label = this.options.dialog.ownerDocument.createElement("label");
    if (
      prompt.interactiveAvailable ||
      prompt.keyInteractiveAvailable ||
      prompt.agentInteractiveAvailable ||
      prompt.passwordInteractiveAvailable
    ) {
      const doc = label.ownerDocument,
        caption = doc.createElement("span");
      method = doc.createElement("select");
      uiText(caption, "认证方式");
      uiAttribute(method, "aria-label", "认证方式");
      const saved = doc.createElement("option"),
        mfa = doc.createElement("option");
      saved.value = "saved";
      uiText(
        saved,
        prompt.agentInteractiveAvailable
          ? "保存的 Agent"
          : prompt.keyInteractiveAvailable
            ? "保存的私钥"
            : "保存的密码",
      );
      mfa.value = prompt.agentInteractiveAvailable
        ? "agent-interactive"
        : prompt.keyInteractiveAvailable
          ? "key-interactive"
          : "interactive";
      uiText(
        mfa,
        prompt.agentInteractiveAvailable
          ? "Agent + MFA"
          : prompt.keyInteractiveAvailable
            ? "私钥 + MFA"
            : "交互认证 / MFA",
      );
      method.append(saved, mfa);
      if (prompt.passwordInteractiveAvailable) {
        const passwordMfa = doc.createElement("option");
        passwordMfa.value = "password-interactive";
        uiText(passwordMfa, "密码 + MFA");
        method.append(passwordMfa);
      }
      method.value =
        prompt.passwordInteractiveAvailable &&
        prompt.index + 1 === prompt.count &&
        this.passwordJointChoices.get(this.options.identity.value) ===
          prompt.identity
          ? "password-interactive"
          : prompt.agentInteractiveAvailable &&
              prompt.index + 1 === prompt.count &&
              this.agentJointChoices.get(this.options.identity.value) ===
                prompt.identity
            ? "agent-interactive"
            : prompt.index + 1 === prompt.count &&
                this.jointChoices.get(this.options.identity.value) ===
                  prompt.identity
              ? "key-interactive"
              : prompt.index + 1 === prompt.count &&
                  this.interactiveChoices.get(this.options.identity.value) ===
                    prompt.identity
                ? "interactive"
                : "saved";
      label.append(caption, method);
      this.options.status.before(label);
    }
    this.options.connect.disabled = false;
    this.options.connect.focus();
    return new Promise((resolve) => {
      const finish = (approved: RouteDecision) => {
        label.parentNode?.removeChild(label);
        signal.removeEventListener("abort", abort);
        this.routeAnswer = undefined;
        this.options.connect.disabled = true;
        uiText(this.options.connect, "连接");
        resolve(approved);
      };
      const abort = () => finish(false);
      this.routeAnswer = () =>
        finish(
          this.current(epoch) && !signal.aborted
            ? method?.value === "password-interactive"
              ? "password-interactive"
              : method?.value === "agent-interactive"
                ? "agent-interactive"
                : method?.value === "key-interactive"
                  ? "key-interactive"
                  : method?.value === "interactive"
                    ? "interactive"
                    : true
            : false,
        );
      signal.addEventListener("abort", abort, { once: true });
    });
  }
  private async connect(): Promise<void> {
    if (this.routeAnswer) {
      this.routeAnswer();
      return;
    }
    const epoch = this.epoch;
    const nonce = this.originalNonce;
    const controller = this.controller;
    if (
      !nonce ||
      !controller ||
      this.busy ||
      this.options.connect.disabled ||
      !this.current(epoch)
    )
      return;
    const passwordJointIdentity = this.passwordJointChoices.get(
      this.options.identity.value,
    );
    const jointIdentity = this.jointChoices.get(this.options.identity.value);
    const keyIdentity =
      this.keyChoices.get(this.options.identity.value) ?? jointIdentity;
    const agentIdentity = this.agentChoices.get(this.options.identity.value);
    const agentJointIdentity = this.agentJointChoices.get(
      this.options.identity.value,
    );
    const interactiveIdentity = this.interactiveChoices.get(
      this.options.identity.value,
    );
    const interactive =
      this.dependencies.connectInteractive ?? connectSavedInteractiveSsh;
    const connect = agentIdentity
      ? this.dependencies.connectAgent
      : keyIdentity
        ? this.dependencies.connectKey
        : this.dependencies.connect;
    if (!connect) return;
    this.busy = true;
    this.options.working(true);
    this.options.connect.disabled = true;
    this.options.fingerprint.disabled = true;
    this.options.host.disabled = true;
    this.options.identity.disabled = true;
    uiText(this.options.status, "正在验证主机并连接保存的身份…");
    try {
      if (!this.choiceUsers.has(this.options.identity.value))
        throw Error("ssh.session_denied");
      const selectedHost = this.hosts.find(
        (host) => host.id === this.options.host.value,
      );
      const route = selectedHost?.profile?.via;
      const id =
        (interactiveIdentity ||
          jointIdentity ||
          agentJointIdentity ||
          passwordJointIdentity) &&
        !route
          ? await (async () => {
              return (
                passwordJointIdentity
                  ? (this.dependencies.connectPasswordInteractive ??
                      connectSavedPasswordInteractiveSsh)
                  : agentJointIdentity
                    ? (this.dependencies.connectAgentInteractive ??
                      connectSavedAgentInteractiveSsh)
                    : jointIdentity
                      ? (this.dependencies.connectKeyInteractive ??
                        connectSavedPrivateKeyInteractiveSsh)
                      : interactive
              )(
                nonce,
                {
                  host: this.options.host.value,
                  identity:
                    passwordJointIdentity ??
                    agentJointIdentity ??
                    jointIdentity ??
                    interactiveIdentity!,
                },
                controller.signal,
                () => this.current(epoch),
                (prompt, signal) =>
                  answerInteractivePrompt(this.options.dialog, prompt, signal),
              );
            })()
          : route && selectedHost?.profile
            ? await (this.dependencies.connectRoute ?? connectSshRoute)(
                nonce,
                {
                  host: selectedHost.id,
                  identity:
                    passwordJointIdentity ??
                    agentJointIdentity ??
                    agentIdentity ??
                    keyIdentity ??
                    interactiveIdentity ??
                    this.options.identity.value,
                  profile: selectedHost.profile.id,
                  privateKey: keyIdentity !== undefined,
                  ...(agentIdentity || agentJointIdentity
                    ? { agent: true }
                    : {}),
                },
                controller.signal,
                () => this.current(epoch),
                (prompt) => this.confirmRoute(prompt, controller.signal, epoch),
              )
            : await connect(
                nonce,
                {
                  host: this.options.host.value,
                  identity:
                    agentIdentity ??
                    keyIdentity ??
                    interactiveIdentity ??
                    this.options.identity.value,
                },
                controller.signal,
                () => this.current(epoch),
              );
      if (!this.current(epoch)) return;
      const temporary = this.temporary;
      this.close(true);
      try {
        await this.options.connected(nonce, id);
      } finally {
        if (
          temporary &&
          !this.disposed &&
          !this.options.signal.aborted &&
          this.options.nonce() === nonce
        )
          this.options.temporaryConnected?.(nonce, temporary);
      }
    } catch (error) {
      if (this.current(epoch)) {
        const code =
          error instanceof Error
            ? error.message
            : typeof error === "string"
              ? error
              : "";
        uiText(
          this.options.status,
          code.includes("peer_failed")
            ? "主机指纹未批准或不匹配，请关闭后检查指纹。"
            : code.includes("uncertain")
              ? "连接结果未确认，请关闭后刷新会话列表；不要重复连接。"
              : "连接未成功，请检查已解锁的保险库、身份凭据和主机；未自动重试。",
        );
        this.options.cancel.focus();
      }
    } finally {
      if (this.current(epoch)) {
        this.busy = false;
        this.options.working(false);
        this.describeHost();
      }
    }
  }
  close(registered = false): void {
    const original = this.originalNonce;
    const temporary = this.temporary;
    this.temporary = undefined;
    if (temporary && original && !registered)
      void cancelTemporaryTarget(original, temporary.token);
    const same = original !== undefined && original === this.options.nonce();
    ++this.epoch;
    this.busy = false;
    this.controller?.abort();
    this.controller = undefined;
    if (this.options.dialog.open) this.options.dialog.close();
    this.options.connect.disabled = true;
    this.options.fingerprint.disabled = true;
    uiText(this.options.connect, "连接");
    this.routeAnswer = undefined;
    this.options.host.replaceChildren();
    this.options.selectedHost.textContent = "";
    this.options.identity.replaceChildren();
    this.jointChoices.clear();
    this.passwordJointChoices.clear();
    this.keyChoices.clear();
    this.agentChoices.clear();
    this.agentJointChoices.clear();
    this.choiceUsers.clear();
    this.hosts = [];
    this.ready = false;
    uiText(this.options.status, "");
    if (same && !this.disposed && !this.options.signal.aborted) {
      this.options.working(false);
      if (this.focus?.isConnected) this.focus.focus();
    }
    this.focus = undefined;
    this.originalNonce = undefined;
  }
}
