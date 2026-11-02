<script lang="ts" generics="T extends string">
  // Сегментный переключатель из макета: дорожка seg + рамка line, radius 9, padding 3;
  // сегмент 28 px (30 в шапке «Проверки»), radius 7, 12/500; выбранный — segon + segshadow.
  let {
    options,
    value,
    label,
    height = 28,
    disabled = false,
    animated = false,
    onchange,
  }: {
    options: { value: T; label: string }[];
    value: T;
    label: string;
    height?: 28 | 30;
    disabled?: boolean;
    animated?: boolean;
    onchange: (v: T) => void;
  } = $props();
  let track: HTMLDivElement;
  let indicator = $state({ x: 0, width: 0 });
  function measure() {
    const selected = track?.querySelector<HTMLButtonElement>('[aria-pressed="true"]');
    if (selected) indicator = { x: selected.offsetLeft, width: selected.offsetWidth };
  }
  $effect(() => {
    if (!animated || !track) return;
    const observer = new ResizeObserver(measure);
    observer.observe(track);
    let mounted = true;
    document.fonts.ready.then(() => { if (mounted) measure(); });
    measure();
    return () => { mounted = false; observer.disconnect(); };
  });
  $effect(() => {
    void value;
    if (!animated) return;
    const frame = requestAnimationFrame(measure);
    return () => cancelAnimationFrame(frame);
  });
</script>

<div class="track" class:animated bind:this={track} role="group" aria-label={label}>
  {#if animated}<span class="indicator" aria-hidden="true" style:width="{indicator.width}px" style:transform="translateX({indicator.x}px)"></span>{/if}
  {#each options as o (o.value)}
    <button
      type="button"
      class="seg"
      class:on={o.value === value}
      style:height="{height}px"
      style:padding="0 {height === 30 ? 14 : 12}px"
      aria-pressed={o.value === value}
      {disabled}
      onclick={() => o.value !== value && onchange(o.value)}>{o.label}</button
    >
  {/each}
</div>

<style>
  .track {
    position: relative;
    display: flex;
    padding: 3px;
    gap: 2px;
    border-radius: 9px;
    background: var(--seg);
    border: 1px solid var(--line);
    flex-shrink: 0;
  }
  .seg {
    position: relative;
    z-index: 1;
    border: none;
    border-radius: 7px;
    font-size: 12px;
    font-weight: 500;
    background: transparent;
    color: var(--dim);
    white-space: nowrap;
  }
  .seg.on {
    background: var(--segon);
    color: var(--tx);
    box-shadow: var(--segshadow);
  }
  .animated .seg.on { background: transparent; box-shadow: none; }
  .indicator {
    position: absolute;
    left: 0;
    top: 3px;
    bottom: 3px;
    border-radius: 7px;
    background: var(--segon);
    box-shadow: var(--segshadow);
    pointer-events: none;
    transition: transform 180ms var(--theme-ease), width 180ms var(--theme-ease);
  }
  .seg:disabled {
    cursor: default;
    opacity: 0.6;
  }
</style>
