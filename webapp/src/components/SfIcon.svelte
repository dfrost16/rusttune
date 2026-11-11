<script>
  let { name, size = 15, class: cls = '' } = $props();
</script>

<svg
  width={size}
  height={size}
  viewBox="0 0 24 24"
  fill="none"
  stroke="currentColor"
  stroke-width="1.85"
  stroke-linecap="round"
  stroke-linejoin="round"
  class={`sf-ico ${cls}`}
  aria-hidden="true"
>
  {#if name === 'music'}
    <path d="M9 18V5l10-2v13" /><circle cx="6" cy="18" r="3" /><circle cx="16" cy="16" r="3" />
  {:else if name === 'search'}
    <circle cx="11" cy="11" r="7" /><path d="m20 20-3.5-3.5" />
  {:else if name === 'plus'}
    <path d="M12 5v14M5 12h14" />
  {:else if name === 'minus'}
    <path d="M5 12h14" />
  {:else if name === 'trash'}
    <path d="M3 6h18" /><path d="M8 6V4a1 1 0 0 1 1-1h6a1 1 0 0 1 1 1v2" />
    <path d="M19 6l-1 14a2 2 0 0 1-2 2H8a2 2 0 0 1-2-2L5 6" />
  {:else if name === 'download'}
    <circle cx="12" cy="12" r="9" /><path d="M12 8v8m0 0-3-3m3 3 3-3" />
  {:else if name === 'square_down'}
    <rect x="3" y="3" width="18" height="18" rx="4" /><path d="M12 8v8m0 0-3-3m3 3 3-3" />
  {:else if name === 'link'}
    <path d="M10 13a5 5 0 0 0 7.5.5l3-3a5 5 0 0 0-7-7l-1.7 1.7" />
    <path d="M14 11a5 5 0 0 0-7.5-.5l-3 3a5 5 0 0 0 7 7l1.7-1.7" />
  {:else if name === 'play'}
    <path fill="currentColor" stroke="none" d="M7 5.2v13.6c0 .8.9 1.3 1.6.9l11.2-6.8c.7-.4.7-1.4 0-1.8L8.6 4.3C7.9 3.9 7 4.4 7 5.2z" />
  {:else if name === 'pause'}
    <rect fill="currentColor" stroke="none" x="6" y="4.5" width="4.4" height="15" rx="1.4" />
    <rect fill="currentColor" stroke="none" x="13.6" y="4.5" width="4.4" height="15" rx="1.4" />
  {:else if name === 'prev'}
    <rect fill="currentColor" stroke="none" x="4" y="5.5" width="2.3" height="13" rx="1" />
    <path fill="currentColor" stroke="none" d="M19 6.2v11.6c0 .8-.9 1.2-1.5.8L8.8 12.8c-.6-.4-.6-1.2 0-1.6l8.7-5.8c.6-.4 1.5 0 1.5.8z" />
  {:else if name === 'next'}
    <rect fill="currentColor" stroke="none" x="17.7" y="5.5" width="2.3" height="13" rx="1" />
    <path fill="currentColor" stroke="none" d="M5 6.2v11.6c0 .8.9 1.2 1.5.8l8.7-5.8c.6-.4.6-1.2 0-1.6L6.5 5.4C5.9 5 5 5.4 5 6.2z" />
  {:else if name === 'repeat'}
    <path d="m17 2 4 4-4 4" /><path d="M3 11v-1a4 4 0 0 1 4-4h14" />
    <path d="m7 22-4-4 4-4" /><path d="M21 13v1a4 4 0 0 1-4 4H3" />
  {:else if name === 'heart'}
    <path fill="currentColor" stroke="none" d="M12 20.6s-7.5-4.7-9.6-9C.8 8.3 2.7 4.5 6.4 4.1c2.2-.2 4.2.9 5.6 2.6 1.4-1.7 3.4-2.8 5.6-2.6 3.7.4 5.6 4.2 4 7.5-2.1 4.3-9.6 9-9.6 9z" />
  {:else if name === 'heart_outline'}
    <path d="M12 20.6s-7.5-4.7-9.6-9C.8 8.3 2.7 4.5 6.4 4.1c2.2-.2 4.2.9 5.6 2.6 1.4-1.7 3.4-2.8 5.6-2.6 3.7.4 5.6 4.2 4 7.5-2.1 4.3-9.6 9-9.6 9z" />
  {:else if name === 'power'}
    <path d="M12 3v8" /><path d="M17.5 6.8a8 8 0 1 1-11 0" />
  {:else if name === 'chevron'}
    <path d="m9 6 6 6-6 6" />
  {:else if name === 'list'}
    <path d="M8 6h12M8 12h12M8 18h12" />
    <circle cx="4" cy="6" r="1" fill="currentColor" stroke="none" />
    <circle cx="4" cy="12" r="1" fill="currentColor" stroke="none" />
    <circle cx="4" cy="18" r="1" fill="currentColor" stroke="none" />
  {:else if name === 'speaker'}
    <path d="M11 5 6 9H3v6h3l5 4V5z" /><path d="M15.5 8.5a5 5 0 0 1 0 7" />
  {:else if name === 'folder'}
    <path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V7z" />
  {:else if name === 'check'}
    <path d="M5 12.5 9.5 17 19 7" />
  {:else if name === 'pencil'}
    <path d="M16.5 3.5a2.1 2.1 0 0 1 3 3L7 19l-4 1 1-4Z" />
  {:else if name === 'share'}
    <path d="M12 3v12m0-12-4 4m4-4 4 4" /><path d="M5 13v6a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2v-6" />
  {:else if name === 'close'}
    <path d="M6 6l12 12M18 6L6 18" />
  {/if}
</svg>

<style>
  .sf-ico {
    display: inline-block;
    vertical-align: middle;
    flex-shrink: 0;
  }
</style>
