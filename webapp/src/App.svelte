<script>
  import { onMount } from 'svelte';
  import SfIcon from './components/SfIcon.svelte';
  import { createDeckStore, fmtBytes, fmtTime } from './state/deck.svelte.js';

  const deck = createDeckStore();

  let cmdInput = $state('');
  let cmdEl = null;
  let newPlName = $state('');
  let showNewPl = $state(false);
  let renameTarget = $state(null); // { id, name }
  let confirmState = $state(null); // { title, body, yesLabel, onYes }
  let customImportName = $state('');
  let pinInput = $state('');
  let pinBusy = $state(false);

  onMount(() => {
    const cleanup = deck.init();
    const onKey = (e) => {
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'k') {
        e.preventDefault();
        cmdEl?.focus();
        cmdEl?.select();
      }
      if (e.key === 'Escape') {
        confirmState = null;
        renameTarget = null;
        showNewPl = false;
      }
    };
    window.addEventListener('keydown', onKey);
    return () => {
      cleanup();
      window.removeEventListener('keydown', onKey);
    };
  });

  const isUrlInput = $derived(
    cmdInput.includes('http') || cmdInput.includes('list=') || cmdInput.includes('youtu')
  );

  const visibleTracks = $derived.by(() => {
    const q = deck.searchFilter.trim().toLowerCase();
    if (!q) return deck.tracks;
    return deck.tracks.filter(
      (t) =>
        t.title.toLowerCase().includes(q) ||
        t.artist.toLowerCase().includes(q) ||
        (t.album || '').toLowerCase().includes(q)
    );
  });

  const activePlaylist = $derived(
    deck.playlists.find((p) => p.id === deck.activePlaylistId) || null
  );

  const selectedBytes = $derived(
    visibleTracks
      .filter((t) => deck.selectedIds.has(t.video_id))
      .reduce((acc, t) => acc + (t.file_bytes || 0), 0)
  );

  function handleCmdInput() {
    if (!isUrlInput) {
      deck.searchFilter = cmdInput;
    } else {
      deck.searchFilter = '';
    }
  }

  async function submitCmd(e) {
    e.preventDefault();
    const val = cmdInput.trim();
    if (!val) return;
    if (isUrlInput) {
      deck.importUrl = val;
      await deck.previewImport(val);
      if (deck.importPreview?.title) {
        customImportName = deck.importPreview.title;
      }
    } else {
      deck.searchFilter = val;
    }
  }

  function askConfirm(title, body, yesLabel, onYes) {
    confirmState = { title, body, yesLabel, onYes };
  }
</script>

<div class="wb-shell">
  <!-- Top Command Header -->
  <header class="wb-top">
    <div class="wb-brand">
      <span class={`led ${deck.serverOnline ? 'on' : 'off'}`}></span>
      <span class="wb-title">RustTune</span>
      <span class="kbd">RG35XX H · {deck.status.battery || 'AC'}</span>
    </div>

    <form class="wb-cmd" onsubmit={submitCmd}>
      <SfIcon name={isUrlInput ? 'link' : 'search'} size={14} class="muted" />
      <input
        bind:this={cmdEl}
        bind:value={cmdInput}
        oninput={handleCmdInput}
        placeholder="Paste YouTube / YTM playlist URL to import, or filter tracks..."
      />
      <span class="kbd">Ctrl+K</span>
      {#if isUrlInput}
        <button type="submit" class="btn accent" disabled={deck.importBusy}>
          {deck.importBusy ? 'Fetching...' : 'Import URL'}
        </button>
      {:else if cmdInput}
        <button
          type="button"
          class="btn"
          onclick={() => {
            cmdInput = '';
            deck.searchFilter = '';
          }}
        >
          Clear
        </button>
      {/if}
    </form>

    <div class="wb-mini-deck">
      <div class="mini-meta">
        <span class="mini-song">{deck.status.title || 'Idle'}</span>
        <span class="mini-time">{deck.status.elapsed_str} / {deck.status.duration_str}</span>
      </div>
      <button class="btn sq" title="Previous" onclick={() => deck.cmd('prev')}>
        <SfIcon name="prev" size={13} />
      </button>
      <button class="btn sq" title="Play / Pause" onclick={() => deck.cmd('toggle')}>
        <SfIcon name={deck.status.playing ? 'pause' : 'play'} size={13} />
      </button>
      <button class="btn sq" title="Next" onclick={() => deck.cmd('next')}>
        <SfIcon name="next" size={13} />
      </button>
      <button
        class={`btn sq ${deck.status.loop_track ? 'accent-outline' : ''}`}
        title="Loop Current Song"
        onclick={() => deck.cmd('loop')}
      >
        <SfIcon name="repeat" size={13} />
      </button>
      <button
        class="btn danger sq"
        title="Shut down local web server"
        onclick={() =>
          askConfirm(
            'Shut down local web server?',
            'Playback on your RG35XX H continues unaffected. You can turn the server back on anytime in handheld Settings.',
            'Turn Off Server',
            () => deck.stopServer()
          )}
      >
        <SfIcon name="power" size={13} />
      </button>
    </div>
  </header>

  <!-- Workspace Sub-bar -->
  <div class="wb-subbar">
    <nav class="chips">
      <button
        class={`chip ${deck.activeTab === 'playlists' ? 'active' : ''}`}
        onclick={() => deck.selectTab('playlists')}
      >
        Playlists ({deck.playlists.length})
      </button>
      <button
        class={`chip ${deck.activeTab === 'downloads' ? 'active' : ''}`}
        onclick={() => deck.selectTab('downloads')}
      >
        Offline Files · {fmtBytes(deck.status.offline_bytes)}
      </button>
      <button
        class={`chip ${deck.activeTab === 'orphans' ? 'active' : ''}`}
        onclick={() => deck.selectTab('orphans')}
      >
        Orphaned Cache ({deck.status.orphan_count})
      </button>
      <button
        class={`chip ${deck.activeTab === 'favorites' ? 'active' : ''}`}
        onclick={() => deck.selectTab('favorites')}
      >
        Favorites ({deck.status.fav_count})
      </button>
      <button
        class={`chip ${deck.activeTab === 'history' ? 'active' : ''}`}
        onclick={() => deck.selectTab('history')}
      >
        History
      </button>
    </nav>

    <div class="wb-tools">
      <button class="btn" title="Capture 640x480 handheld screenshot" onclick={() => deck.downloadScreenshot()}>
        <SfIcon name="camera" size={13} />
        Screenshot
      </button>
      <button class="btn" onclick={() => (showNewPl = !showNewPl)}>
        <SfIcon name="plus" size={13} />
        New Playlist
      </button>
      <a class="btn" href="/api/export" download="rusttune-library-backup.json">Export JSON</a>
    </div>
  </div>

  <main class="wb-body">
    <!-- New Playlist Inline Creator -->
    {#if showNewPl}
      <form
        class="wb-banner"
        onsubmit={async (e) => {
          e.preventDefault();
          await deck.createPlaylist(newPlName);
          newPlName = '';
          showNewPl = false;
        }}
      >
        <input
          class="wb-input"
          bind:value={newPlName}
          placeholder="New playlist name..."
        />
        <button type="submit" class="btn accent">Create</button>
        <button type="button" class="btn" onclick={() => (showNewPl = false)}>Cancel</button>
      </form>
    {/if}

    <!-- Live YouTube Playlist Import Preview Drawer -->
    {#if deck.importPreview}
      <section class="wb-import-card">
        <div class="wb-import-head">
          <div>
            <div class="mono muted" style="font-size:11px;">YOUTUBE PLAYLIST PREVIEW</div>
            <div style="display:flex;gap:8px;align-items:center;margin-top:4px;">
              <input class="wb-input" bind:value={customImportName} style="min-width:260px;" />
              <span class="mono muted">
                {deck.importPreview.tracks.length} tracks · ~{deck.importPreview.tracks.length * 4} MB offline
              </span>
            </div>
          </div>
          <div style="display:flex;gap:8px;align-items:center;">
            <button
              class="btn"
              disabled={deck.importBusy}
              onclick={async () => {
                await deck.commitImport(false, customImportName);
                cmdInput = '';
              }}
            >
              Add to Library
            </button>
            <button
              class="btn accent"
              disabled={deck.importBusy}
              onclick={async () => {
                await deck.commitImport(true, customImportName);
                cmdInput = '';
              }}
            >
              <SfIcon name="download" size={13} />
              Import &amp; Download All
            </button>
            <button class="btn sq" onclick={() => (deck.importPreview = null)}>
              <SfIcon name="close" size={13} />
            </button>
          </div>
        </div>
      </section>
    {/if}

    <!-- Playlist Cards Strip when in Playlists tab -->
    {#if deck.activeTab === 'playlists'}
      <section class="pl-strip">
        {#each deck.playlists as pl (pl.id)}
          <div
            class={`pl-card ${pl.id === deck.activePlaylistId ? 'active' : ''}`}
            role="button"
            tabindex="0"
            onclick={() => deck.selectPlaylist(pl.id)}
            onkeydown={(e) => e.key === 'Enter' && deck.selectPlaylist(pl.id)}
          >
            <div class="pl-art">
              <SfIcon name="music" size={16} />
            </div>
            <div style="flex:1;min-width:0;">
              <div class="pl-name">{pl.name}</div>
              <div class="mono muted" style="font-size:11px;">
                {pl.track_count} tracks · {fmtBytes(pl.offline_bytes)}
              </div>
            </div>
          </div>
        {/each}
      </section>

      {#if activePlaylist}
        <div class="pl-actions-bar">
          <div>
            <strong>{activePlaylist.name}</strong>
            <span class="mono muted" style="margin-left:8px;font-size:12px;">
              {visibleTracks.length} tracks · {fmtBytes(activePlaylist.offline_bytes)} offline
            </span>
          </div>
          <div style="display:flex;gap:7px;">
            <button class="btn" onclick={() => deck.playPlaylistOnHandheld(activePlaylist.id)}>
              <SfIcon name="play" size={12} /> Play All
            </button>
            <button
              class="btn"
              onclick={() => (renameTarget = { id: activePlaylist.id, name: activePlaylist.name })}
            >
              <SfIcon name="pencil" size={12} /> Rename
            </button>
            <button
              class="btn danger"
              onclick={() =>
                askConfirm(
                  `Delete "${activePlaylist.name}"?`,
                  'Removes this playlist from your library. Tracks stay in Favorites/Downloads unless deleted individually.',
                  'Delete Playlist',
                  () => deck.deletePlaylist(activePlaylist.id, false)
                )}
            >
              <SfIcon name="trash" size={12} /> Delete Playlist
            </button>
          </div>
        </div>
      {/if}
    {/if}

    {#if renameTarget}
      <form
        class="wb-banner"
        onsubmit={async (e) => {
          e.preventDefault();
          await deck.renamePlaylist(renameTarget.id, renameTarget.name);
          renameTarget = null;
        }}
      >
        <span class="mono muted">RENAME:</span>
        <input class="wb-input" bind:value={renameTarget.name} />
        <button type="submit" class="btn accent">Save</button>
        <button type="button" class="btn" onclick={() => (renameTarget = null)}>Cancel</button>
      </form>
    {/if}

    <!-- Orphaned Cache Action Header -->
    {#if deck.activeTab === 'orphans'}
      <div class="pl-actions-bar">
        <div>
          <strong>Orphaned Offline Files</strong>
          <span class="mono muted" style="margin-left:8px;font-size:12px;">
            {visibleTracks.length} cached .m4a files not in any playlist or favorites ({fmtBytes(deck.status.orphan_bytes)})
          </span>
        </div>
        {#if visibleTracks.length > 0}
          <button
            class="btn danger"
            onclick={() =>
              askConfirm(
                `Clean ${visibleTracks.length} orphaned files?`,
                `Frees ${fmtBytes(deck.status.orphan_bytes)} on your RG35XX H SD card without touching saved playlists or favorites.`,
                'Clean Orphaned',
                () => deck.cleanOrphans()
              )}
          >
            <SfIcon name="trash" size={12} /> Clean All Orphaned
          </button>
        {/if}
      </div>
    {/if}

    <!-- Batch-Selectable Dense Table -->
    <div class="wb-table">
      <div class="tr head">
        <input
          type="checkbox"
          checked={visibleTracks.length > 0 && deck.selectedIds.size === visibleTracks.length}
          onchange={() => deck.selectAllVisible(visibleTracks)}
          aria-label="Select all"
        />
        <span></span>
        <span>TRACK / ARTIST</span>
        <span>STORAGE STATUS</span>
        <span style="text-align:right;">SIZE</span>
        <span style="text-align:right;">ACTIONS</span>
      </div>

      {#if visibleTracks.length === 0}
        <div class="empty-row muted">
          No tracks in this view yet. Paste a YouTube playlist URL above or switch tabs.
        </div>
      {:else}
        {#each visibleTracks as t (t.video_id)}
          <div class={`tr ${deck.status.video_id === t.video_id ? 'playing' : ''}`}>
            <input
              type="checkbox"
              checked={deck.selectedIds.has(t.video_id)}
              onchange={() => deck.toggleSelected(t.video_id)}
            />
            <img
              class="thumb"
              src={`/api/cover/${encodeURIComponent(t.video_id)}`}
              alt=""
              loading="lazy"
              onerror={(e) => { e.currentTarget.style.visibility = 'hidden'; }}
            />
            <div style="min-width:0;">
              <div class="track-name">{t.title}</div>
              <div class="muted track-sub">
                {t.artist}{t.album ? ` · ${t.album}` : ''} · {fmtTime(t.duration_secs)}
              </div>
            </div>
            <div>
              {#if t.local_path}
                <span class="status-ok">Cached Offline</span>
              {:else}
                <span class="muted">Stream Only</span>
              {/if}
            </div>
            <div class="mono muted" style="text-align:right;">
              {t.file_bytes > 0 ? fmtBytes(t.file_bytes) : '—'}
            </div>
            <div class="row-btns">
              <button class="btn sq" title="Play" onclick={() => deck.playTrack(t)}>
                <SfIcon name="play" size={12} />
              </button>
              {#if deck.activeTab === 'playlists' && activePlaylist}
                <button
                  class="btn sq"
                  title="Remove from playlist"
                  onclick={() => deck.removeFromPlaylist(activePlaylist.id, t.video_id)}
                >
                  <SfIcon name="minus" size={12} />
                </button>
              {/if}
              {#if t.local_path}
                <button
                  class="btn danger sq"
                  title="Delete offline .m4a file"
                  onclick={() =>
                    askConfirm(
                      'Delete offline file?',
                      `Delete "${t.title}" (${fmtBytes(t.file_bytes)}) from RG35XX H storage?`,
                      'Delete File',
                      () => deck.deleteOfflineFile(t.video_id)
                    )}
                >
                  <SfIcon name="trash" size={12} />
                </button>
              {:else}
                <button
                  class="btn sq"
                  title="Download offline to RG35XX H"
                  onclick={() => deck.queueDownload(t.video_id)}
                >
                  <SfIcon name="download" size={12} />
                </button>
              {/if}
            </div>
          </div>
        {/each}
      {/if}
    </div>

    <!-- Bulk Floating Selection Bar -->
    {#if deck.selectedIds.size > 0}
      <div class="bulk-bar">
        <span>
          <strong>{deck.selectedIds.size} selected</strong>
          <span class="mono muted"> ({fmtBytes(selectedBytes)} offline)</span>
        </span>
        <div style="display:flex;gap:8px;">
          <button class="btn" onclick={() => deck.selectAllVisible([])}>Deselect</button>
          <button
            class="btn danger"
            onclick={() =>
              askConfirm(
                `Delete ${deck.selectedIds.size} offline files?`,
                `Free ${fmtBytes(selectedBytes)} from RG35XX H SD card? Playlists stay intact.`,
                'Delete Selected',
                () => deck.bulkDeleteFiles(deck.selectedIds)
              )}
          >
            <SfIcon name="trash" size={13} /> Delete Offline Files
          </button>
        </div>
      </div>
    {/if}
  </main>

  {#if deck.toastText}
    <div class="toast">{deck.toastText}</div>
  {/if}

  {#if deck.authRequired}
    <div class="modal-bg">
      <form
        class="modal-card"
        onsubmit={async (e) => {
          e.preventDefault();
          if (pinInput.trim().length < 6 || pinBusy) return;
          pinBusy = true;
          const ok = await deck.verifyPin(pinInput);
          pinBusy = false;
          if (!ok) pinInput = '';
        }}
      >
        <div class="mono muted" style="font-size:11px;margin-bottom:6px;">SECURITY PAIRING</div>
        <h3>Enter 6-Digit Handheld PIN</h3>
        <p class="muted">Read the 6-digit code shown on your RG35XX H in <strong>Find</strong> or <strong>Settings</strong>.</p>
        <input
          class="wb-input mono"
          type="password"
          inputmode="numeric"
          maxlength="6"
          placeholder="000000"
          bind:value={pinInput}
          oninput={async () => {
            pinInput = pinInput.replace(/\D/g, '').slice(0, 6);
            if (pinInput.length === 6 && !pinBusy) {
              pinBusy = true;
              const ok = await deck.verifyPin(pinInput);
              pinBusy = false;
              if (!ok) pinInput = '';
            }
          }}
          style="width:100%;font-size:22px;letter-spacing:0.35em;text-align:center;margin-bottom:10px;"
        />
        {#if deck.authError}
          <div style="color:#f87171;font-size:12px;margin-bottom:10px;">{deck.authError}</div>
        {/if}
        <div class="modal-actions">
          <button type="submit" class="btn accent" style="width:100%;" disabled={pinBusy || pinInput.length < 6}>
            {pinBusy ? 'Verifying...' : 'Unlock Workbench'}
          </button>
        </div>
      </form>
    </div>
  {/if}

  {#if confirmState}
    <div class="modal-bg" role="presentation" onclick={(e) => e.target === e.currentTarget && (confirmState = null)}>
      <div class="modal-card">
        <h3>{confirmState.title}</h3>
        <p class="muted">{confirmState.body}</p>
        <div class="modal-actions">
          <button class="btn" onclick={() => (confirmState = null)}>Cancel</button>
          <button
            class="btn danger"
            onclick={async () => {
              const fn = confirmState.onYes;
              confirmState = null;
              await fn();
            }}
          >
            {confirmState.yesLabel}
          </button>
        </div>
      </div>
    </div>
  {/if}
</div>

<style>
  :global(html, body) {
    margin: 0;
    padding: 0;
    background: #0d0e11;
    color: #f4f4f6;
    font-family: "Inter", "SF Pro Text", -apple-system, BlinkMacSystemFont, "Segoe UI", system-ui, sans-serif;
    -webkit-font-smoothing: antialiased;
  }
  :global(*) { box-sizing: border-box; }
  .mono { font-family: ui-monospace, "SF Mono", Menlo, Consolas, monospace; font-variant-numeric: tabular-nums; }
  .muted { color: #8c92a4; }

  .wb-shell { min-height: 100vh; display: flex; flex-direction: column; }
  .wb-top {
    position: sticky; top: 0; z-index: 30;
    background: #101216;
    border-bottom: 1px solid #20232c;
    padding: 10px 20px;
    display: flex; align-items: center; justify-content: space-between; gap: 14px; flex-wrap: wrap;
  }
  .wb-brand { display: flex; align-items: center; gap: 9px; }
  .wb-title { font-size: 13.5px; font-weight: 700; letter-spacing: -0.015em; }
  .led { width: 8px; height: 8px; border-radius: 50%; }
  .led.on { background: #22c55e; box-shadow: 0 0 8px rgba(34,197,94,0.6); }
  .led.off { background: #ef4444; }
  .kbd {
    font-family: ui-monospace, monospace; font-size: 10.5px; padding: 2px 7px;
    border-radius: 5px; background: #1a1d26; border: 1px solid #2b303d; color: #9ca3af;
  }

  .wb-cmd {
    flex: 1; max-width: 560px;
    display: flex; align-items: center; gap: 8px;
    background: #14161c; border: 1px solid #2a2e3a; border-radius: 8px; padding: 5px 10px;
  }
  .wb-cmd:focus-within { border-color: #6366f1; }
  .wb-cmd input {
    flex: 1; background: transparent; border: 0; outline: 0; color: #f4f4f6;
    font-family: inherit; font-size: 13px;
  }

  .wb-mini-deck { display: flex; align-items: center; gap: 6px; }
  .mini-meta { display: flex; flex-direction: column; align-items: flex-end; margin-right: 4px; max-width: 170px; }
  .mini-song { font-size: 12px; font-weight: 600; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; max-width: 170px; }
  .mini-time { font-size: 10.5px; color: #8c92a4; font-family: ui-monospace, monospace; }

  .wb-subbar {
    padding: 9px 20px; background: #0f1115; border-bottom: 1px solid #1d2029;
    display: flex; align-items: center; justify-content: space-between; gap: 12px; flex-wrap: wrap;
  }
  .chips { display: flex; gap: 6px; flex-wrap: wrap; }
  .chip {
    background: transparent; border: 1px solid transparent; color: #8c92a4;
    padding: 5px 11px; border-radius: 6px; font-family: inherit; font-size: 12px; font-weight: 500; cursor: pointer;
  }
  .chip:hover { color: #f4f4f6; }
  .chip.active { background: #1a1d25; border-color: #2d3240; color: #fff; font-weight: 600; }
  .wb-tools { display: flex; gap: 7px; align-items: center; }

  .btn {
    display: inline-flex; align-items: center; justify-content: center; gap: 5px;
    border: 1px solid #2b303c; background: #171a22; color: #e5e7eb;
    border-radius: 6px; padding: 5px 10px; font-family: inherit; font-size: 12px; font-weight: 600;
    cursor: pointer; text-decoration: none;
  }
  .btn:hover { border-color: #3e4556; background: #1e222d; }
  .btn.sq { padding: 5px 7px; }
  .btn.accent { background: #6366f1; border-color: #6366f1; color: #fff; }
  .btn.accent-outline { border-color: #6366f1; color: #a5b4fc; background: rgba(99,102,241,0.14); }
  .btn.danger { background: rgba(239,68,68,0.14); border-color: rgba(239,68,68,0.38); color: #f87171; }
  .btn:disabled { opacity: 0.55; cursor: not-allowed; }

  .wb-body {
    max-width: 1280px; width: 100%; margin: 0 auto;
    padding: 18px 20px 80px; display: flex; flex-direction: column; gap: 14px;
  }
  .wb-banner, .wb-import-card {
    background: #13161e; border: 1px solid #292d3a; border-radius: 9px; padding: 11px 14px;
    display: flex; align-items: center; gap: 10px; flex-wrap: wrap;
  }
  .wb-import-head { display: flex; justify-content: space-between; align-items: center; width: 100%; gap: 12px; flex-wrap: wrap; }
  .wb-input {
    background: #0c0d11; border: 1px solid #2d3240; border-radius: 6px; padding: 6px 10px;
    color: #fff; font-family: inherit; font-size: 13px; outline: 0;
  }

  .pl-strip { display: grid; grid-template-columns: repeat(auto-fill, minmax(210px, 1fr)); gap: 9px; }
  .pl-card {
    background: #12141a; border: 1px solid #212530; border-radius: 9px; padding: 10px 12px;
    display: flex; align-items: center; gap: 10px; cursor: pointer;
  }
  .pl-card:hover { border-color: #353b4c; }
  .pl-card.active { border-color: #6366f1; background: rgba(99,102,241,0.10); }
  .pl-art {
    width: 34px; height: 34px; border-radius: 7px; background: #1e222e; color: #a5b4fc;
    display: grid; place-items: center; flex-shrink: 0;
  }
  .pl-name { font-size: 13px; font-weight: 600; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }

  .pl-actions-bar {
    display: flex; align-items: center; justify-content: space-between; gap: 10px; flex-wrap: wrap;
    padding: 4px 2px;
  }

  .wb-table { border: 1px solid #1f222b; border-radius: 10px; overflow: hidden; background: #101216; }
  .tr {
    display: grid;
    grid-template-columns: 28px 34px minmax(0, 1.5fr) 130px 76px 110px;
    gap: 10px; align-items: center;
    padding: 8px 12px; border-bottom: 1px solid #1a1d24; font-size: 12.5px;
  }
  .tr.head { font-size: 11px; font-weight: 700; color: #71788a; background: #13151b; }
  .tr:last-child { border-bottom: 0; }
  .tr:not(.head):hover { background: #151820; }
  .tr.playing { background: rgba(99, 102, 241, 0.09); }
  .thumb { width: 28px; height: 28px; border-radius: 5px; object-fit: cover; background: #1b1e26; }
  .track-name { font-weight: 600; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .track-sub { font-size: 11.5px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .status-ok { color: #34d399; font-weight: 600; font-size: 12px; }
  .row-btns { display: flex; justify-content: flex-end; gap: 5px; }
  .empty-row { padding: 28px; text-align: center; font-size: 13px; }

  .bulk-bar {
    position: sticky; bottom: 16px;
    display: flex; align-items: center; justify-content: space-between; gap: 12px;
    padding: 11px 15px; border-radius: 10px;
    background: #171b26; border: 1px solid #3c4563;
    box-shadow: 0 12px 32px rgba(0,0,0,0.55);
  }

  .toast {
    position: fixed; right: 20px; bottom: 20px; z-index: 60;
    background: #1b1e28; border: 1px solid #363c4e; border-radius: 8px;
    padding: 10px 14px; font-size: 12.5px; font-weight: 600;
    box-shadow: 0 12px 30px rgba(0,0,0,0.5);
  }

  .modal-bg {
    position: fixed; inset: 0; z-index: 80; background: rgba(0,0,0,0.62);
    display: grid; place-items: center; padding: 20px;
  }
  .modal-card {
    width: 100%; max-width: 380px; background: #15171e; border: 1px solid #2b303e;
    border-radius: 12px; padding: 18px;
  }
  .modal-card h3 { margin: 0 0 6px; font-size: 15px; }
  .modal-card p { margin: 0 0 16px; font-size: 13px; line-height: 1.45; }
  .modal-actions { display: flex; justify-content: flex-end; gap: 8px; }
</style>
