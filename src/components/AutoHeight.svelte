<script lang="ts">
  import { onMount, type Snippet } from "svelte";
  let { children }: { children: Snippet } = $props();
  let content: HTMLDivElement;
  let height = $state<number | null>(null);
  onMount(() => {
    const observer = new ResizeObserver(() => { height = content.offsetHeight; });
    observer.observe(content);
    return () => observer.disconnect();
  });
</script>

<div class="height" style:height={height === null ? "auto" : `${height}px`}>
  <div bind:this={content}>{@render children()}</div>
</div>

<style>
  .height { overflow: hidden; transition: height 180ms var(--theme-ease); }
</style>
