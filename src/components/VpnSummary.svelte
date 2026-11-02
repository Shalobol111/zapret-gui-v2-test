<script lang="ts">
  import Button from "./Button.svelte";
  import Spinner from "./Spinner.svelte";
  import * as api from "../lib/api";
  import { app, go } from "../lib/state.svelte";
  import { toast } from "../lib/ui.svelte";
  const vpn = $derived(app.status?.vpn);
  let pending = $state(false);
  const busy = $derived(pending || !!vpn?.busy);
  const configured = $derived(!!vpn?.installed && !!vpn.xrayInstalled && vpn.servers > 0);
  const title = $derived(busy ? "VPN — переключение…" : vpn?.running ? vpn.ready ? "VPN включён" : "VPN — подключение не готово" : "VPN выключен");
  async function toggle() {
    if (busy) return;
    pending = true;
    try { if (vpn?.running) await api.vpnStop(); else await api.vpnStart(); }
    catch (e) { toast(api.errorText(e), "err"); }
    finally { pending = false; }
  }
</script>

<section class="vpn-summary" aria-label="Состояние VPN">
  <span class="indicator" class:on={vpn?.running && vpn.ready} class:err={!!vpn?.error && !vpn.running}>
    {#if busy}<Spinner size={10} />{/if}
  </span>
  <span class="title">{title}</span>
  <span class="detail" title={vpn?.error ?? vpn?.server ?? undefined}>{vpn?.error ?? (vpn?.running ? vpn.server : configured ? "Готов к подключению" : "Добавьте подписку или ключ")}</span>
  <div class="actions">
    {#if configured || vpn?.running}
      <Button height={30} size={12} disabled={busy || (!app.isAdmin && !vpn?.running) || !!app.status?.testsRunning} onclick={toggle}>{vpn?.running ? "Выключить" : "Включить"}</Button>
    {/if}
    <button type="button" class="link" onclick={() => go("vpn")}>{configured ? "Настроить" : "Настроить VPN"}</button>
  </div>
</section>

<style>
  .vpn-summary { display: flex; align-items: center; gap: 10px; min-height: 46px; flex-shrink: 0; padding: 7px 16px; border: 1px solid var(--line); border-radius: 12px; background: var(--card); }
  .indicator { display: inline-flex; width: 10px; height: 10px; align-items: center; justify-content: center; border-radius: 50%; background: var(--mute); flex-shrink: 0; }
  .indicator.on { background: var(--accent); }
  .indicator.err { background: var(--err); }
  .title { font-size: 13px; font-weight: 500; }
  .detail { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-size: 12px; color: var(--faint); }
  .actions { display: flex; align-items: center; gap: 12px; margin-left: auto; flex-shrink: 0; }
  .link { border: 0; background: none; padding: 0; font-size: 12px; color: var(--accent); }
  @media (max-width: 650px) { .detail { display: none; } .vpn-summary { flex-wrap: wrap; } }
</style>
