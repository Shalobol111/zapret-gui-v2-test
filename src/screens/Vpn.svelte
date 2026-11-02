<script lang="ts">
  // VPN вместе с zapret (PLAN.md §17): сайты из VPN-списков — через VPN-сервер (sing-box),
  // остальное напрямую, где работает zapret. Карточка состояния — как на Главной.
  import { slide } from "svelte/transition";
  import { cubicInOut } from "svelte/easing";
  import { onMount, tick } from "svelte";
  import Spinner from "../components/Spinner.svelte";
  import AutoHeight from "../components/AutoHeight.svelte";
  import Button from "../components/Button.svelte";
  import PageHeader from "../components/PageHeader.svelte";
  import PowerButton from "../components/PowerButton.svelte";
  import Segmented from "../components/Segmented.svelte";
  import Switch from "../components/Switch.svelte";
  import * as api from "../lib/api";
  import { app, openList } from "../lib/state.svelte";
  import { duration, toast } from "../lib/ui.svelte";
  import type { VpnInbound, VpnRouteMode, VpnServer, VpnStore, VpnView } from "../lib/types";

  const fail = (e: unknown) => toast(api.errorText(e), "err");
  const vs = $derived(app.status?.vpn);
  /** Оба ядра на месте: sing-box (маршрутизация) и Xray (VLESS/VMess/Trojan/Shadowsocks). */
  const installed = $derived(!!vs?.installed && !!vs?.xrayInstalled);
  const running = $derived(!!vs?.running);

  let view = $state<VpnView | null>(null);
  let routes = $state("");
  let readingRoutes = $state(false);
  async function readRoutes() {
    if (readingRoutes) return;
    readingRoutes = true;
    try { routes = await api.vpnRoutes(); } catch (e) { fail(e); }
    finally { readingRoutes = false; }
  }
  let pendingInbound = $state<VpnInbound | null>(null);
  let advancedOpen = $state(false);
  let reducedMotion = $state(false);
  const motionDuration = $derived(reducedMotion ? 0 : 180);
  const store = $derived(view?.store);
  const inbound = $derived(pendingInbound ?? store?.inbound ?? "proxy");
  const routeRows = $derived(routes.split("\n").filter(Boolean).map((line) => {
    const [route, ...parts] = line.split(" | ");
    return { route: parts.length ? route : "", text: parts.length ? parts.join(" · ") : line };
  }));
  onMount(() => {
    const media = matchMedia("(prefers-reduced-motion: reduce)");
    const updateMotion = () => { reducedMotion = media.matches; };
    updateMotion();
    media.addEventListener("change", updateMotion);
    return () => media.removeEventListener("change", updateMotion);
  });
  async function changeInbound(next: VpnInbound) {
    if (busy || !store || next === inbound) return;
    busy = "set";
    pendingInbound = next;
    await tick();
    try { view = await api.vpnSet({ inbound: next }); }
    catch (e) { await load(); fail(e); }
    finally { pendingInbound = null; busy = null; }
  }
  async function load() {
    try {
      view = await api.vpnView();
    } catch (e) {
      fail(e);
    }
  }
  let loaded = false;
  $effect(() => {
    if (app.screen === "vpn" && !loaded) {
      loaded = true;
      load();
    }
  });

  // ── Действия ─────────────────────────────────────────────────
  let busy = $state<null | "power" | "install" | "add" | "update" | "ping" | "set">(null);
  async function run(kind: NonNullable<typeof busy>, fn: () => Promise<unknown>) {
    if (busy) return;
    busy = kind;
    try {
      await fn();
    } catch (e) {
      fail(e);
    } finally {
      busy = null;
    }
  }

  const toggle = () => run("power", () => (running ? api.vpnStop() : api.vpnStart()));
  const install = () => run("install", async () => toast(await api.vpnInstall(), "ok"));

  let input = $state("");
  const add = () =>
    run("add", async () => {
      const before = allServers.length;
      view = await api.vpnAdd(input);
      input = "";
      toast(`Добавлено серверов: ${Math.max(0, allServers.length - before)}`, "ok");
    });
  const updateSub = (id: string | null) =>
    run("update", async () => {
      view = await api.vpnUpdate(id);
      toast("Подписка обновлена", "ok");
    });
  const remove = (id: string) => run("set", async () => (view = await api.vpnRemove(id)));
  const select = (id: string | null) => run("set", async () => (view = await api.vpnSelect(id)));
  const set = (patch: Partial<VpnStore>) => run("set", async () => (view = await api.vpnSet(patch)));
  const ping = () =>
    run("ping", async () => {
      const pings = await api.vpnPing();
      if (view) view.pings = pings;
      toast(`HTTPS-проверка: доступно ${Object.values(pings).filter((v) => v !== null).length} из ${Object.keys(pings).length} серверов`, "info");
    });

  // ── Тексты ───────────────────────────────────────────────────
  const allServers = $derived<VpnServer[]>(
    store ? [...store.subscriptions.flatMap((s) => s.servers), ...store.manual] : [],
  );
  const usable = $derived(allServers.filter((s) => !s.unsupported));
  /** Через Xray: протоколы, которые он ведёт (как в Rust: Server::via_xray). */
  const viaXray = (s: VpnServer) => ["VLESS", "VMess", "Trojan", "Shadowsocks"].includes(s.protocol) && s.link !== null;
  let now = $state(Date.now());
  $effect(() => {
    if (!running || !vs?.startedAt) return;
    now = Date.now();
    const t = setInterval(() => (now = Date.now()), 1000);
    return () => clearInterval(t);
  });
  const modeLabel: Record<VpnRouteMode, string> = { lists: "списки → VPN", all_except_ru: "всё через VPN, кроме РФ" };
  const inboundLabel: Record<VpnInbound, string> = { proxy: "системный прокси", tun: "все программы (TUN)" };
  const title = $derived(
    vs?.busy ? `${vs.busy}…` : running ? (vs?.ready ? "VPN включён" : "Запуск…") : "VPN выключен",
  );
  const subtitle = $derived.by(() => {
    if (!vs) return "";
    if (!installed)
      return vs.installed
        ? "Нужен ещё Xray — ядро для серверов VLESS/VMess/Trojan/Shadowsocks (около 15 МБ)"
        : "Нужны ядра VPN: sing-box и Xray (около 46 МБ с GitHub)";
    if (running) {
      const up = vs.startedAt ? ` · ${duration(now - vs.startedAt)}` : "";
      return `${vs.server ?? "сервер"} · ${vs.routeMode ? modeLabel[vs.routeMode] : ""} · ${vs.inbound ? inboundLabel[vs.inbound] : ""}${vs.viaXray ? " · через Xray" : ""}${up}`;
    }
    if (!vs.servers) return "Добавьте подписку или ключ — ниже, в «Серверах»";
    return "Сайты из VPN-списков идут через VPN, остальные — напрямую через zapret";
  });
  const gb = (b: number) => (b / 1024 ** 3).toFixed(b >= 10 * 1024 ** 3 ? 0 : 1);
  const date = (s: number) => new Date(s * 1000).toLocaleDateString("ru-RU");
  const time = (ms: number) => new Date(ms).toLocaleString("ru-RU", { day: "2-digit", month: "2-digit", hour: "2-digit", minute: "2-digit" });
  function subLine(s: VpnStore["subscriptions"][number]) {
    const parts = [`серверов: ${s.servers.length}`];
    if (s.info?.total) parts.push(`трафик ${gb(s.info.upload + s.info.download)} из ${gb(s.info.total)} ГБ`);
    if (s.info?.expire) parts.push(`до ${date(s.info.expire)}`);
    if (s.updatedAt) parts.push(`обновлено ${time(s.updatedAt)}`);
    return parts.join(" · ");
  }
  const pingText = (id: string) => {
    const p = view?.pings[id];
    return p === undefined ? "" : p === null ? "проверка не пройдена" : `${p} мс · HTTPS`;
  };
</script>

<PageHeader title="VPN">
  <span class="head-status">
    {#if installed}sing-box {vs?.version ?? ""} · Xray {vs?.xrayVersion ?? ""}{#if vs?.latest}&nbsp;·&nbsp;<button
          type="button"
          class="link upd"
          title={`Доступно: ${vs.latest}`}
          disabled={!!busy || !!vs.busy}
          onclick={install}>обновить</button
        >{/if}{:else}ядра VPN · не скачаны{/if}
  </span>
</PageHeader>

<section class="card state" aria-label="Состояние VPN">
  <PowerButton
    on={running}
    busy={busy === "power" || !!vs?.busy}
    disabled={!installed || !usable.length || !!busy || !app.isAdmin}
    title={!app.isAdmin ? "Нужны права администратора" : !installed ? "Сначала скачайте ядра VPN" : !usable.length ? "Добавьте подписку или ключ" : undefined}
    onclick={toggle}
  />
  <div class="state-body">
    <div class="titles">
      <div class="title-row">
        <span class="dot" class:on={running && vs?.ready}></span>
        <span class="title">{title}</span>
      </div>
      <span class="sub">{subtitle}</span>
      {#if vs?.error && !running}<span class="note err">{vs.error}</span>{/if}
    </div>
    {#if !installed}
      {#if vs?.busy}
        <div class="bar" aria-hidden="true"><span style:width="{vs.progress ?? 6}%"></span></div>
        <span class="note">{vs.busy}{vs.progress != null ? ` · ${vs.progress}%` : ""}</span>
      {:else}
        <div class="actions"><Button variant="primary" disabled={!!busy} onclick={install}>Скачать ядра</Button></div>
      {/if}
    {/if}
  </div>
</section>

<section class="card" aria-labelledby="vpn-srv">
  <div class="card-head">
    <h2 id="vpn-srv">Серверы</h2>
    {#if allServers.length}
      <Button height={30} size={12} disabled={!!busy} onclick={ping}>{busy === "ping" ? "Проверка…" : "Проверить подключение"}</Button>
    {/if}
  </div>
  <form
    class="add"
    onsubmit={(e) => {
      e.preventDefault();
      if (input.trim()) add();
    }}
  >
    <input
      class="mono"
      spellcheck="false"
      placeholder="Ссылка на подписку (https://…) или ключ (vless://, ss://, trojan://…)"
      aria-label="Подписка или ключ"
      bind:value={input}
    />
    <Button variant="primary" height={34} disabled={!input.trim() || !!busy} onclick={add}
      >{busy === "add" ? "Загрузка…" : "Добавить"}</Button
    >
  </form>

  {#each store?.subscriptions ?? [] as s (s.id)}
    <div class="sub-row">
      <div class="text">
        <span class="label">{s.name}</span>
        <span class="hint">{subLine(s)}</span>
        {#if s.error}<span class="hint err">Не обновилась: {s.error}</span>{/if}
      </div>
      <button type="button" class="link" disabled={!!busy} onclick={() => updateSub(s.id)}>Обновить</button>
      <button type="button" class="link danger" disabled={!!busy} onclick={() => remove(s.id)}>Удалить</button>
    </div>
  {/each}

  {#if allServers.length}
    <ul class="servers" role="radiogroup" aria-label="Сервер">
      <li>
        <label class="srv" class:on={!store?.selected}>
          <input type="radio" name="vpn-srv" checked={!store?.selected} disabled={!!busy} onchange={() => select(null)} />
          <span class="srv-name">Автоматически — самый быстрый</span>
          <span class="srv-meta">из {Math.min(usable.length, 30)}</span>
        </label>
      </li>
      {#each allServers as s (s.id)}
        <li>
          <label class="srv" class:on={store?.selected === s.id} class:off={!!s.unsupported} title={s.unsupported ?? `${s.host}:${s.port}`}>
            <input
              type="radio"
              name="vpn-srv"
              checked={store?.selected === s.id}
              disabled={!!s.unsupported || !!busy}
              onchange={() => select(s.id)}
            />
            <span class="srv-name">{s.name}</span>
            <span class="badge" title={viaXray(s) ? "Соединение — через Xray" : "Соединение — через sing-box"}>{s.protocol}</span>
            <span class="srv-meta" class:err={view?.pings[s.id] === null}>{s.unsupported ? "не поддерживается" : pingText(s.id)}</span>
            {#if store?.manual.some((m) => m.id === s.id)}
              <button type="button" class="link danger x" aria-label="Удалить сервер" disabled={!!busy} onclick={() => remove(s.id)}>×</button>
            {/if}
          </label>
        </li>
      {/each}
    </ul>
  {:else}
    <p class="hint pad">
      Вставьте ссылку на подписку от вашего VPN-сервиса или ключ. Подписки обновляются по кнопке; ключи хранятся только на
      этом компьютере (%APPDATA%\ZapretGUI\vpn).
    </p>
  {/if}
</section>

<div class="row2">
  <section class="card small" aria-labelledby="vpn-route">
    <h2 id="vpn-route">Что идёт через VPN</h2>
    <Segmented
      label="Режим маршрутизации"
      value={store?.routeMode ?? "lists"}
      disabled={!store || !!busy}
      options={[
        { value: "lists", label: "Списки → VPN" },
        { value: "all_except_ru", label: "Всё, кроме РФ" },
      ]}
      onchange={(v) => set({ routeMode: v })}
    />
    {#if store?.routeMode === "all_except_ru"}
      <p class="hint">Российские сайты (.ru, .рф, сервисы, не пускающие из-за границы) — напрямую, всё остальное — через VPN.</p>
    {:else}
      <p class="hint">Через VPN — сайты из list-vpn-general и своего списка, остальное — напрямую. Изменить домены можно в разделе «Списки».</p>
    {/if}
    <div class="line">
      <div class="text">
        <span class="label">Сайты zapret — напрямую</span>
        <span class="hint">YouTube, Discord и всё из списков zapret идут мимо VPN, через zapret</span>
      </div>
      <Switch label="Сайты zapret — напрямую" checked={!!store?.zapretDirect} disabled={!store || !!busy} onchange={(v) => set({ zapretDirect: v })} />
    </div>
    <div class="links">
      <button type="button" class="link" onclick={() => openList("vpn_general")}>list-vpn-general</button>
      <button type="button" class="link" onclick={() => openList("vpn_domains")}>Свой список «через VPN»</button>
      <button type="button" class="link" onclick={() => openList("vpn_direct")}>«Всегда напрямую»</button>
    </div>
  </section>

  <section class="card small" aria-labelledby="vpn-conn">
    <h2 id="vpn-conn">Подключение</h2>
    <div class="mode-row" aria-busy={pendingInbound !== null}>
      <Segmented
        label="Какие программы"
        value={inbound}
        animated
        disabled={!store || !!busy}
        options={[
          { value: "proxy", label: "Системный прокси" },
          { value: "tun", label: "Все программы (TUN)" },
        ]}
        onchange={changeInbound}
      />
      <span class="mode-progress" role="status" aria-label={pendingInbound ? "Переключение режима подключения" : undefined}>
        {#if pendingInbound}<Spinner size={16} />{/if}
      </span>
    </div>
    <AutoHeight><p class="hint">
      {inbound === "tun"
        ? "Весь трафик всех программ, включая Discord и игры. Экспериментально: проверьте, что zapret и VPN работают вместе."
        : "Браузеры и программы, которые используют прокси Windows. Без драйвера; при выключении прокси возвращается как было."}
    </p></AutoHeight>
    {#if inbound !== "tun"}
      <div transition:slide={{ duration: motionDuration, easing: cubicInOut }}>
        <div class="line">
          <span class="label">Порт прокси</span>
          <input class="mono port" inputmode="numeric" aria-label="Порт прокси"
            value={store?.port ?? 2080} disabled={!store || !!busy}
            onchange={(e) => {
              const n = Number(e.currentTarget.value);
              if (Number.isInteger(n) && n >= 1024 && n <= 65535) set({ port: n });
              else toast("Порт — число от 1024 до 65535", "err");
            }} />
        </div>
      </div>
    {/if}
    <div class="line">
      <div class="text">
        <span class="label">Включать вместе с ZapretGUI</span>
      </div>
      <Switch label="Включать вместе с ZapretGUI" checked={!!store?.autostart} disabled={!store || !!busy} onchange={(v) => set({ autostart: v })} />
    </div>
  </section>
</div>

<section class="card advanced" aria-label="Расширенные параметры VPN">
  <button type="button" class="advanced-head" aria-expanded={advancedOpen} aria-controls="vpn-advanced-body"
    onclick={() => { advancedOpen = !advancedOpen; }}>
    <svg class="chevron" class:open={advancedOpen} width="16" height="16" viewBox="0 0 16 16" fill="none" aria-hidden="true"><path d="m6 4 4 4-4 4" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" /></svg>
    <span>Расширенные параметры VPN</span>
  </button>
  {#if advancedOpen}
    <div id="vpn-advanced-body" transition:slide={{ duration: motionDuration, easing: cubicInOut }}>
      <div class="advanced-body">
        {#if inbound === "tun"}
          <div class="line" transition:slide={{ duration: motionDuration, easing: cubicInOut }}>
            <div class="text">
              <span class="label">Другой способ обработки TUN</span>
              <span class="hint">Для проверки проблем с подключением. Переключение перезапустит VPN.</span>
            </div>
            <Switch label="Другой способ обработки TUN" checked={!!store?.systemStack} disabled={!store || !!busy} onchange={(v) => set({ systemStack: v })} />
          </div>
        {/if}
        <div class="links"><button type="button" class="link" onclick={() => api.openPath("vpnlog").catch(fail)}>Лог sing-box</button></div>
        <section class="route-console" aria-labelledby="vpn-routes-title">
          <div class="console-head">
            <h2 id="vpn-routes-title">Фактические маршруты</h2>
            <Button disabled={!running || readingRoutes || !!busy} onclick={readRoutes}>
              <span class="refresh-label">Обновить снимок<span class="refresh-progress" role="status" aria-label={readingRoutes ? "Обновление маршрутов" : undefined}>{#if readingRoutes}<Spinner />{/if}</span></span>
            </Button>
          </div>
          <p class="hint console-hint">Снимок активных соединений. «Напрямую» означает обход VPN; участие zapret этим не подтверждается.</p>
          <!-- svelte-ignore a11y_no_noninteractive_tabindex (Keyboard users must be able to scroll the snapshot.) -->
          <div class="console-body" role="region" aria-label="Снимок маршрутов" aria-busy={readingRoutes} tabindex="0">
            {#if !running}<div class="console-empty">Включите VPN, чтобы посмотреть активные соединения.</div>{/if}
            {#each routeRows as row, i (i)}
              <div class="route-row" class:message={!row.route}>
                {#if row.route}<span class="route-label" class:direct={row.route === "Напрямую"}>{row.route}</span>{/if}
                <span class="route-text">{row.text}</span>
              </div>
            {:else}
              {#if running}<div class="console-empty">{readingRoutes ? "Получение снимка…" : "Обновите снимок во время просмотра видео или подключения к голосу."}</div>{/if}
            {/each}
          </div>
        </section>
      </div>
    </div>
  {/if}
</section>

<style>
  .mode-row { display: flex; align-items: center; gap: 12px; min-width: 0; }
  .mode-progress, .refresh-progress { display: inline-flex; align-items: center; justify-content: center; width: 18px; min-width: 18px; }
  .refresh-label { display: inline-flex; align-items: center; gap: 6px; }
  .advanced { overflow: hidden; }
  .advanced-head { display: flex; align-items: center; gap: 10px; width: 100%; min-height: 44px; padding: 10px 20px; border: none; background: transparent; color: var(--tx); font-size: 14px; font-weight: 600; text-align: left; }
  .advanced-head:focus-visible { outline-offset: -3px; }
  .chevron { flex-shrink: 0; transition: transform 180ms var(--theme-ease); }
  .chevron.open { transform: rotate(90deg); }
  .advanced-body { display: flex; flex-direction: column; gap: 16px; padding: 16px 20px 20px; border-top: 1px solid var(--line); }
  .advanced-body .text { gap: 4px; }
  .advanced-body .links { margin-top: 0; }
  .route-console { min-width: 0; border: 1px solid var(--line); border-radius: 12px; overflow: hidden; container-type: inline-size; }
  .console-head { display: flex; align-items: center; justify-content: space-between; flex-wrap: wrap; gap: 12px; padding: 12px 20px; }
  .console-hint { padding: 0 20px 12px; }
  .console-body { min-height: 160px; max-height: 320px; overflow: auto; padding: 8px 20px 14px; border-top: 1px solid var(--line); font: 12px/22px var(--font-mono); user-select: text; cursor: text; }
  .route-row { display: grid; grid-template-columns: 100px minmax(0, 1fr); gap: 8px; }
  .route-row.message { display: block; }
  .route-label { color: var(--tx); }
  .route-label.direct { color: var(--accent); }
  .route-text { color: var(--tx2); overflow-wrap: anywhere; white-space: pre-wrap; }
  .console-empty { color: var(--faint); overflow-wrap: anywhere; }
  @container (max-width: 420px) {
    .route-row { grid-template-columns: minmax(0, 1fr); gap: 0; margin-bottom: 8px; }
  }
  .head-status {
    font-size: 12px;
    color: var(--faint);
  }
  .link {
    border: none;
    background: none;
    padding: 0;
    color: var(--accent);
    font-size: 12px;
  }
  .link:disabled {
    opacity: 0.45;
    cursor: default;
  }
  .link.upd {
    color: var(--warn);
  }
  .link.danger {
    color: var(--err);
  }
  .card {
    border-radius: 16px;
    background: var(--card);
    border: 1px solid var(--line);
    flex-shrink: 0;
  }
  .state {
    display: flex;
    align-items: center;
    gap: 28px;
    padding: 24px 28px;
  }
  .state-body {
    flex-grow: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 14px;
  }
  .titles {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .title-row {
    display: flex;
    align-items: center;
    gap: 10px;
  }
  .dot {
    width: 8px;
    height: 8px;
    border-radius: 4px;
    background: var(--mute);
    flex-shrink: 0;
  }
  .dot.on {
    background: var(--accent);
  }
  .title {
    font-size: 22px;
    font-weight: 600;
    letter-spacing: -0.01em;
  }
  .sub {
    font-size: 13px;
    color: var(--dim);
    font-variant-numeric: tabular-nums;
  }
  .note {
    font-size: 12px;
    color: var(--faint);
  }
  .note.err,
  .hint.err,
  .srv-meta.err {
    color: var(--err);
  }
  .actions {
    display: flex;
    gap: 10px;
  }
  .bar {
    height: 6px;
    max-width: 380px;
    border-radius: 3px;
    background: var(--raised);
    overflow: hidden;
  }
  .bar span {
    display: block;
    height: 100%;
    background: var(--accent);
    transition: width 0.3s ease;
  }
  .card-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 16px 20px 0;
  }
  h2 {
    font-size: 14px;
    font-weight: 600;
  }
  .add {
    display: flex;
    gap: 10px;
    padding: 12px 20px;
  }
  .add input {
    flex-grow: 1;
    min-width: 0;
  }
  input:not([type]),
  input.port {
    box-sizing: border-box;
    height: 34px;
    padding: 0 10px;
    border-radius: 9px;
    border: 1px solid var(--line2);
    background: var(--raised);
    color: var(--tx);
  }
  input.port {
    width: 90px;
  }
  input:focus {
    outline: none;
    border-color: var(--accent);
  }
  .mono {
    font-family: var(--font-mono);
    font-size: 12px;
  }
  .sub-row {
    display: flex;
    align-items: center;
    gap: 14px;
    padding: 8px 20px;
    border-top: 1px solid var(--line);
  }
  .text {
    flex-grow: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .label {
    font-size: 13px;
  }
  .hint {
    margin: 0;
    font-size: 12px;
    line-height: 1.5;
    color: var(--faint);
  }
  .hint.pad {
    padding: 0 20px 16px;
  }
  .servers {
    list-style: none;
    margin: 0;
    padding: 4px 12px 12px;
    max-height: 280px;
    overflow: auto;
    border-top: 1px solid var(--line);
  }
  .srv {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 7px 8px;
    border-radius: 8px;
    font-size: 13px;
    cursor: pointer;
  }
  .srv:hover {
    background: var(--raised);
  }
  .srv.on {
    background: var(--raised);
  }
  .srv.off {
    opacity: 0.5;
    cursor: default;
  }
  .srv input {
    accent-color: var(--accent);
  }
  .srv-name {
    flex-grow: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .badge {
    font-size: 11px;
    padding: 1px 6px;
    border-radius: 5px;
    background: var(--raised);
    border: 1px solid var(--line2);
    color: var(--dim);
  }
  .srv-meta {
    width: 110px;
    flex-shrink: 0;
    overflow-wrap: anywhere;
    text-align: right;
    font-size: 12px;
    color: var(--faint);
    font-variant-numeric: tabular-nums;
  }
  .x {
    font-size: 16px;
    line-height: 1;
  }
  .row2 {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(min(100%, 380px), 1fr));
    gap: 16px;
    align-items: stretch;
  }
  .small {
    flex: 1 1 0;
    min-width: 0;
    padding: 18px 20px;
    display: flex;
    flex-direction: column;
    gap: 12px;
  }
  .line {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
  }
  .links {
    margin-top: auto;
    display: flex;
    gap: 16px;
    flex-wrap: wrap;
  }
</style>
