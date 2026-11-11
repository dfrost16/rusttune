<script>
  import { onMount } from 'svelte';
  import SfIcon from './components/SfIcon.svelte';
  import { createDeckStore, fmtBytes, fmtTime } from './state/deck.svelte.js';

  const deck = createDeckStore();

  let tab = $state('playlists'); // now | playlists | import | files
  let editMode = $state(false);
  let showNewPl = $state(false);
  let newPlName = $state('');
  let customImportName = $state('');
  let showInstallGuide = $state(false);
  let installDismissed = $state(
    typeof localStorage !== 'undefined' && localStorage.getItem('pwa_prompt_dismissed') === 'true'
  );
  let phoneTheme = $state(
    (typeof localStorage !== 'undefined' && localStorage.getItem('ytm_phone_theme')) || 'amoled'
  );
  let confirmState = $state(null);
  let pinDigits = $state('');
  let pinBusy = $state(false);

  function cycleTheme() {
    haptic();
    const order = ['amoled', 'dark', 'light'];
    phoneTheme = order[(order.indexOf(phoneTheme) + 1) % order.length];
    try {
      localStorage.setItem('ytm_phone_theme', phoneTheme);
    } catch {}
  }

  function dismissInstall() {
    installDismissed = true;
    try {
      localStorage.setItem('pwa_prompt_dismissed', 'true');
    } catch {}
  }

  async function tapPinDigit(d) {
    haptic();
    if (pinBusy) return;
    if (d === 'del') {
      pinDigits = pinDigits.slice(0, -1);
      return;
    }
    if (pinDigits.length >= 6) return;
    pinDigits += d;
    if (pinDigits.length === 6) {
      pinBusy = true;
      const ok = await deck.verifyPin(pinDigits);
      pinBusy = false;
      if (!ok) pinDigits = '';
    }
  }

  onMount(() => {
    return deck.init();
  });

  function haptic() {
    try {
      navigator.vibrate?.(12);
    } catch {}
  }

  function askConfirm(title, body, yesLabel, onYes) {
    haptic();
    confirmState = { title, body, yesLabel, onYes };
  }

  const activePlaylist = $derived(
    deck.playlists.find((p) => p.id === deck.activePlaylistId) || null
  );
</script>

<div class="ios-app" data-theme={phoneTheme}>
  <!-- PWA Install Banner (only shown when installable or on iOS, dismissible like annie-desk) -->
  {#if !deck.isStandalone && !installDismissed && (deck.isIOS || deck.deferredInstallPrompt)}
    <div class="pwa-banner">
      <div class="pwa-badge"><SfIcon name="music" size={18} /></div>
      <div style="flex:1;min-width:0;">
        <div style="font-size:12.5px;font-weight:700;">Install RustTune</div>
        <div class="sub" style="font-size:11px;">
          {deck.isIOS ? 'Tap Share then Add to Home Screen' : 'Add to Home Screen for full-screen app'}
        </div>
      </div>
      <button
        class="ios-btn primary sm"
        onclick={async () => {
          haptic();
          if (deck.deferredInstallPrompt) {
            const ok = await deck.triggerPwaInstall();
            if (ok) dismissInstall();
          } else {
            showInstallGuide = true;
          }
        }}
      >
        Install
      </button>
      <button class="icon-act" aria-label="Dismiss" onclick={dismissInstall}>
        <SfIcon name="close" size={15} />
      </button>
    </div>
  {/if}

  <main class="ios-scroll">
    {#if tab === 'playlists'}
      <div class="ios-head">
        <h1>Library</h1>
        <div style="display:flex;gap:7px;">
          <button class="ios-btn sec sm" onclick={() => (showNewPl = !showNewPl)}>
            <SfIcon name="plus" size={13} /> New
          </button>
          <button class="ios-btn sec sm" onclick={() => (editMode = !editMode)}>
            {editMode ? 'Done' : 'Edit'}
          </button>
        </div>
      </div>

      {#if showNewPl}
        <form
          class="ios-card row-form"
          onsubmit={async (e) => {
            e.preventDefault();
            await deck.createPlaylist(newPlName);
            newPlName = '';
            showNewPl = false;
          }}
        >
          <input class="ios-input" bind:value={newPlName} placeholder="Playlist name..." />
          <button type="submit" class="ios-btn primary sm">Add</button>
        </form>
      {/if}

      <!-- Playlists List -->
      <div class="sec-label">PLAYLISTS</div>
      <div class="ios-list">
        {#each deck.playlists as pl (pl.id)}
          <div class="ios-row">
            <button
              class="row-main"
              onclick={() => {
                haptic();
                deck.selectPlaylist(pl.id);
              }}
            >
              <div class={`art-sq ${pl.id === deck.activePlaylistId ? 'active' : ''}`}>
                <SfIcon name="music" size={16} />
              </div>
              <div style="flex:1;min-width:0;text-align:left;">
                <div class="row-title">{pl.name}</div>
                <div class="sub">{pl.track_count} tracks · {fmtBytes(pl.offline_bytes)} offline</div>
              </div>
              <SfIcon name="chevron" size={14} class="muted" />
            </button>
            {#if editMode}
              <button
                class="del-pill"
                onclick={() =>
                  askConfirm(
                    `Delete "${pl.name}"?`,
                    'Removes this playlist from your library.',
                    'Delete',
                    () => deck.deletePlaylist(pl.id, false)
                  )}
              >
                Delete
              </button>
            {/if}
          </div>
        {/each}
      </div>

      <!-- Active Playlist Tracks -->
      {#if activePlaylist}
        <div class="sec-label" style="display:flex;justify-content:space-between;align-items:center;">
          <span>{activePlaylist.name.toUpperCase()} ({deck.tracks.length})</span>
          <button class="link-btn" onclick={() => deck.playPlaylistOnHandheld(activePlaylist.id)}>
            Play All
          </button>
        </div>
        <div class="ios-list">
          {#each deck.tracks as t (t.video_id)}
            <div class="ios-row">
              <button class="row-main" onclick={() => deck.playTrack(t)}>
                <img
                  class="art-img"
                  src={`/api/cover/${encodeURIComponent(t.video_id)}`}
                  alt=""
                  loading="lazy"
                  onerror={(e) => { e.currentTarget.style.opacity = '0.2'; }}
                />
                <div style="flex:1;min-width:0;text-align:left;">
                  <div class="row-title">{t.title}</div>
                  <div class="sub">
                    {t.artist} · {t.local_path ? fmtBytes(t.file_bytes) : fmtTime(t.duration_secs)}
                  </div>
                </div>
              </button>
              {#if editMode}
                <button
                  class="icon-act"
                  aria-label="Remove from playlist"
                  onclick={() => deck.removeFromPlaylist(activePlaylist.id, t.video_id)}
                >
                  <SfIcon name="minus" size={14} />
                </button>
                {#if t.local_path}
                  <button
                    class="del-pill"
                    onclick={() =>
                      askConfirm(
                        'Delete Offline File?',
                        `Remove ${t.title} (${fmtBytes(t.file_bytes)}) from RG35XX H?`,
                        'Delete File',
                        () => deck.deleteOfflineFile(t.video_id)
                      )}
                  >
                    File
                  </button>
                {/if}
              {/if}
            </div>
          {/each}
        </div>
      {/if}

    {:else if tab === 'import'}
      <div class="ios-head"><h1>Import</h1></div>
      <div class="ios-card">
        <div style="font-size:14px;font-weight:600;margin-bottom:8px;">YouTube / YT Music Playlist</div>
        <div class="sub" style="margin-bottom:10px;">
          Paste any public playlist or album link to pull into your handheld library.
        </div>
        <form
          class="row-form"
          onsubmit={async (e) => {
            e.preventDefault();
            await deck.previewImport();
            if (deck.importPreview?.title) customImportName = deck.importPreview.title;
          }}
        >
          <input
            class="ios-input"
            bind:value={deck.importUrl}
            placeholder="https://music.youtube.com/playlist?list=..."
          />
          <button type="submit" class="ios-btn primary sm" disabled={deck.importBusy}>
            {deck.importBusy ? '...' : 'Fetch'}
          </button>
        </form>
      </div>

      {#if deck.importPreview}
        <div class="ios-card">
          <div class="sec-label" style="margin-bottom:6px;">PREVIEW · {deck.importPreview.tracks.length} TRACKS</div>
          <input class="ios-input" bind:value={customImportName} style="width:100%;margin-bottom:10px;" />
          <div style="display:flex;gap:8px;">
            <button
              class="ios-btn sec"
              style="flex:1;"
              disabled={deck.importBusy}
              onclick={async () => {
                await deck.commitImport(false, customImportName);
                tab = 'playlists';
              }}
            >
              Save Playlist
            </button>
            <button
              class="ios-btn primary"
              style="flex:1;"
              disabled={deck.importBusy}
              onclick={async () => {
                await deck.commitImport(true, customImportName);
                tab = 'playlists';
              }}
            >
              Import &amp; Cache
            </button>
          </div>
        </div>
      {/if}

    {:else if tab === 'files'}
      <div class="ios-head">
        <h1>Offline</h1>
        {#if deck.status.orphan_count > 0}
          <button
            class="ios-btn danger sm"
            onclick={() =>
              askConfirm(
                `Clean ${deck.status.orphan_count} unused files?`,
                `Frees ${fmtBytes(deck.status.orphan_bytes)} on RG35XX H without removing playlist or favorite tracks.`,
                'Clean',
                () => deck.cleanOrphans()
              )}
          >
            Clean ({deck.status.orphan_count})
          </button>
        {/if}
      </div>

      <div class="ios-card">
        <div style="display:flex;justify-content:space-between;align-items:baseline;">
          <span style="font-size:20px;font-weight:700;">{fmtBytes(deck.status.offline_bytes)}</span>
          <span class="sub">{deck.status.offline_count} cached .m4a files</span>
        </div>
      </div>

      <div class="ios-list">
        {#each deck.tracks as t (t.video_id)}
          <div class="ios-row">
            <button class="row-main" onclick={() => deck.playTrack(t)}>
              <img
                class="art-img"
                src={`/api/cover/${encodeURIComponent(t.video_id)}`}
                alt=""
                loading="lazy"
                onerror={(e) => { e.currentTarget.style.opacity = '0.2'; }}
              />
              <div style="flex:1;min-width:0;text-align:left;">
                <div class="row-title">{t.title}</div>
                <div class="sub">{t.artist} · {fmtBytes(t.file_bytes)}</div>
              </div>
            </button>
            <button
              class="del-pill"
              onclick={() =>
                askConfirm(
                  'Delete File?',
                  `Delete "${t.title}" (${fmtBytes(t.file_bytes)}) from RG35XX H?`,
                  'Delete',
                  () => deck.deleteOfflineFile(t.video_id)
                )}
            >
              Delete
            </button>
          </div>
        {/each}
      </div>

    {:else}
      <!-- Now Playing Full Tab -->
      <div class="ios-head">
        <h1>Now</h1>
        <div style="display:flex;gap:7px;">
          <button class="ios-btn sec sm" aria-label="Capture handheld screenshot" onclick={() => deck.downloadScreenshot()}>
            <SfIcon name="camera" size={14} />
          </button>
          <button class="ios-btn sec sm" onclick={cycleTheme}>
            {phoneTheme === 'amoled' ? 'AMOLED' : phoneTheme === 'light' ? 'Light' : 'Dark'}
          </button>
          <button
            class="ios-btn danger sm"
            onclick={() =>
              askConfirm(
                'Stop Web Server?',
                'Music keeps playing on your RG35XX H. Turn the server back on anytime from handheld Settings.',
                'Turn Off',
                () => deck.stopServer()
              )}
          >
            <SfIcon name="power" size={13} /> Server Off
          </button>
        </div>
      </div>

      <div class="ios-card now-hero">
        {#if deck.status.video_id}
          <img class="hero-art" src={`/api/cover/${encodeURIComponent(deck.status.video_id)}`} alt="" />
        {:else}
          <div class="hero-art placeholder"><SfIcon name="music" size={38} /></div>
        {/if}
        <h2 class="hero-title">{deck.status.title || 'Nothing Playing'}</h2>
        <div class="hero-sub">{deck.status.artist || 'Ready on RG35XX H'}</div>
        <div class="hero-time">
          {deck.status.elapsed_str} / {deck.status.duration_str}
        </div>
        <div class="transport">
          <span class="t-spacer" aria-hidden="true"></span>
          <button class="icon-act" aria-label="Previous" onclick={() => deck.cmd('prev')}>
            <SfIcon name="prev" size={20} />
          </button>
          <button class="play-big" aria-label="Play or Pause" onclick={() => deck.cmd('toggle')}>
            <SfIcon name={deck.status.playing ? 'pause' : 'play'} size={22} />
          </button>
          <button class="icon-act" aria-label="Next" onclick={() => deck.cmd('next')}>
            <SfIcon name="next" size={20} />
          </button>
          <button
            class={`icon-act ${deck.status.loop_track ? 'loop-on' : ''}`}
            aria-label="Loop Track"
            onclick={() => deck.cmd('loop')}
          >
            <SfIcon name="repeat" size={18} />
          </button>
        </div>
      </div>
    {/if}
  </main>

  <!-- Frosted Bottom Mini-Player + iOS Tab Bar -->
  <div class="ios-bottom">
    {#if tab !== 'now'}
      <div
        class="ios-mini"
        role="button"
        tabindex="0"
        onclick={() => (tab = 'now')}
        onkeydown={(e) => e.key === 'Enter' && (tab = 'now')}
      >
        {#if deck.status.video_id}
          <img class="mini-art" src={`/api/cover/${encodeURIComponent(deck.status.video_id)}`} alt="" />
        {:else}
          <div class="mini-art placeholder"><SfIcon name="music" size={15} /></div>
        {/if}
        <div style="flex:1;min-width:0;">
          <div class="row-title" style="font-size:13px;">{deck.status.title || 'Ready to Play'}</div>
          <div class="sub" style="font-size:11px;">{deck.status.artist || 'RG35XX H'}</div>
        </div>
        <button
          class="icon-act"
          onclick={(e) => {
            e.stopPropagation();
            deck.cmd('toggle');
          }}
        >
          <SfIcon name={deck.status.playing ? 'pause' : 'play'} size={16} />
        </button>
        <button
          class="icon-act"
          onclick={(e) => {
            e.stopPropagation();
            deck.cmd('next');
          }}
        >
          <SfIcon name="next" size={15} />
        </button>
      </div>
    {/if}

    <nav class="ios-tabs">
      <button class={`ios-tab ${tab === 'now' ? 'active' : ''}`} onclick={() => (tab = 'now')}>
        <SfIcon name="play" size={18} /> Now
      </button>
      <button
        class={`ios-tab ${tab === 'playlists' ? 'active' : ''}`}
        onclick={() => {
          tab = 'playlists';
          deck.selectTab('playlists');
        }}
      >
        <SfIcon name="music" size={18} /> Playlists
      </button>
      <button class={`ios-tab ${tab === 'import' ? 'active' : ''}`} onclick={() => (tab = 'import')}>
        <SfIcon name="square_down" size={18} /> Import
      </button>
      <button
        class={`ios-tab ${tab === 'files' ? 'active' : ''}`}
        onclick={() => {
          tab = 'files';
          deck.selectTab('downloads');
        }}
      >
        <SfIcon name="folder" size={18} /> Files
      </button>
    </nav>
  </div>

  {#if deck.toastText}
    <div class="ios-toast">{deck.toastText}</div>
  {/if}

  {#if showInstallGuide}
    <div class="modal-bg" role="presentation" onclick={(e) => e.target === e.currentTarget && (showInstallGuide = false)}>
      <div class="apple-alert">
        <div class="alert-body">
          <h3>Install on Home Screen</h3>
          <p>
            {#if deck.isIOS}
              Tap Safari’s <strong>Share</strong> button, then choose <strong>Add to Home Screen</strong>.
            {:else}
              Open your browser menu (<strong>⋮</strong>) and tap <strong>Add to Home screen</strong> or <strong>Install app</strong>.
            {/if}
          </p>
        </div>
        <div class="alert-actions single">
          <button onclick={() => (showInstallGuide = false)}>Got It</button>
        </div>
      </div>
    </div>
  {/if}

  {#if deck.authRequired}
    <div class="ios-pin-lock">
      <div class="pin-card">
        <div class="pwa-badge" style="margin:0 auto 12px;"><SfIcon name="music" size={20} /></div>
        <h2 style="margin:0 0 4px;font-family:var(--font-display);font-size:20px;">Enter Handheld PIN</h2>
        <div class="sub" style="white-space:normal;margin-bottom:18px;">
          Enter the 6-digit code shown on your RG35XX H
        </div>
        <div class="pin-dots">
          {#each [0, 1, 2, 3, 4, 5] as idx}
            <span class={`pdot ${idx < pinDigits.length ? 'filled' : ''}`}></span>
          {/each}
        </div>
        {#if deck.authError}
          <div style="color:var(--red);font-size:12.5px;margin:10px 0 0;">{deck.authError}</div>
        {/if}
        <div class="keypad">
          {#each ['1','2','3','4','5','6','7','8','9'] as d}
            <button class="kp-btn" onclick={() => tapPinDigit(d)}>{d}</button>
          {/each}
          <div></div>
          <button class="kp-btn" onclick={() => tapPinDigit('0')}>0</button>
          <button class="kp-btn ghost" onclick={() => tapPinDigit('del')}>⌫</button>
        </div>
      </div>
    </div>
  {/if}

  {#if confirmState}
    <div class="modal-bg" role="presentation" onclick={(e) => e.target === e.currentTarget && (confirmState = null)}>
      <div class="apple-alert">
        <div class="alert-body">
          <h3>{confirmState.title}</h3>
          <p>{confirmState.body}</p>
        </div>
        <div class="alert-actions">
          <button onclick={() => (confirmState = null)}>Cancel</button>
          <button
            class="destructive"
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
  :global(*) { box-sizing: border-box; }
  :global(html, body) {
    margin: 0;
    padding: 0;
    width: 100%;
    overflow-x: hidden;
    background: #000000;
  }
  .ios-app {
    --font-display: "SF Pro Display", -apple-system, BlinkMacSystemFont, system-ui, sans-serif;
    --font-text: "SF Pro Text", -apple-system, BlinkMacSystemFont, system-ui, sans-serif;
    --accent: #fa2d48;
    --red: #ff453a;
    --blue: #0a84ff;
    --bg: #0b0b0e;
    --pane: rgba(28, 28, 32, 0.78);
    --elevated: rgba(44, 44, 48, 0.86);
    --dock: rgba(24, 24, 28, 0.88);
    --hair: rgba(255, 255, 255, 0.09);
    --t1: #ffffff;
    --t2: rgba(235, 235, 245, 0.68);
    min-height: 100vh;
    background: radial-gradient(circle at 15% 10%, rgba(250,45,72,0.16), transparent 44%), var(--bg);
    color: var(--t1);
    font-family: var(--font-text);
    -webkit-font-smoothing: antialiased;
  }
  .ios-app[data-theme="amoled"] {
    --bg: #000000;
    --pane: #0a0a0c;
    --elevated: #141418;
    --dock: rgba(0, 0, 0, 0.94);
    --hair: rgba(255, 255, 255, 0.11);
    --t1: #ffffff;
    --t2: rgba(235, 235, 245, 0.68);
    background: #000000;
  }
  .ios-app[data-theme="light"] {
    --bg: #f2f2f7;
    --pane: rgba(255, 255, 255, 0.86);
    --elevated: rgba(255, 255, 255, 0.96);
    --dock: rgba(249, 249, 251, 0.90);
    --hair: rgba(60, 60, 67, 0.12);
    --t1: #1c1c1e;
    --t2: rgba(60, 60, 67, 0.70);
    background: var(--bg);
  }
  .sub { font-size: 12px; color: var(--t2); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }

  .pwa-banner {
    margin: env(safe-area-inset-top, 10px) 14px 4px;
    padding: 10px 12px; border-radius: 16px;
    background: linear-gradient(135deg, rgba(250,45,72,0.18), rgba(94,92,230,0.16));
    border: 1px solid rgba(250,45,72,0.34);
    display: flex; align-items: center; gap: 10px;
  }
  .pwa-badge {
    width: 36px; height: 36px; border-radius: 10px;
    background: linear-gradient(145deg, #fa2d48, #bf5af2); color: #fff;
    display: grid; place-items: center; flex-shrink: 0;
  }

  .ios-scroll { padding: 10px 16px 150px; display: flex; flex-direction: column; gap: 12px; }
  .ios-head { display: flex; align-items: center; justify-content: space-between; gap: 10px; }
  .ios-head h1 { font-family: var(--font-display); font-size: 28px; font-weight: 700; letter-spacing: -0.03em; margin: 0; }
  .sec-label { font-size: 11px; font-weight: 600; color: var(--t2); letter-spacing: 0.04em; padding: 0 4px; }

  .ios-btn {
    display: inline-flex; align-items: center; justify-content: center; gap: 5px;
    border: 0; border-radius: 10px; padding: 9px 14px; font-family: inherit; font-size: 13px; font-weight: 600; cursor: pointer;
  }
  .ios-btn.sm { padding: 6px 11px; font-size: 12px; border-radius: 8px; }
  .ios-btn.primary { background: var(--accent); color: #fff; }
  .ios-btn.sec { background: rgba(118,118,128,0.22); color: var(--t1); }
  .ios-btn.danger { background: rgba(255,69,58,0.16); color: var(--red); }
  .link-btn { background: transparent; border: 0; color: var(--accent); font-family: inherit; font-size: 12px; font-weight: 600; }

  .ios-card {
    background: var(--pane); backdrop-filter: blur(22px);
    border: 1px solid var(--hair); border-radius: 14px; padding: 13px;
  }
  .row-form { display: flex; gap: 8px; }
  .ios-input {
    flex: 1; background: rgba(118,118,128,0.18); border: 1px solid var(--hair); border-radius: 9px;
    padding: 8px 11px; color: var(--t1); font-family: inherit; font-size: 13px; outline: 0;
  }

  .ios-list {
    background: var(--pane); backdrop-filter: blur(22px);
    border: 1px solid var(--hair); border-radius: 14px; overflow: hidden;
  }
  .ios-row { display: flex; align-items: center; border-bottom: 1px solid var(--hair); }
  .ios-row:last-child { border-bottom: 0; }
  .row-main {
    flex: 1; min-width: 0; display: flex; align-items: center; gap: 11px;
    padding: 10px 12px; border: 0; background: transparent; color: var(--t1); font-family: inherit; cursor: pointer;
  }
  .art-sq {
    width: 40px; height: 40px; border-radius: 10px; background: rgba(250,45,72,0.15); color: var(--accent);
    display: grid; place-items: center; flex-shrink: 0;
  }
  .art-sq.active { background: var(--accent); color: #fff; }
  .art-img { width: 40px; height: 40px; border-radius: 8px; object-fit: cover; background: rgba(118,118,128,0.2); flex-shrink: 0; }
  .row-title { font-size: 14px; font-weight: 600; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .del-pill {
    margin-right: 10px; border: 0; border-radius: 8px; padding: 7px 11px;
    background: var(--red); color: #fff; font-family: inherit; font-size: 12px; font-weight: 600; cursor: pointer;
  }
  .icon-act {
    border: 0; background: transparent; color: var(--t1); padding: 0;
    width: 40px; height: 40px; display: inline-grid; place-items: center; cursor: pointer;
  }
  .icon-act.loop-on { color: var(--accent); }

  .now-hero {
    width: 100%; display: flex; flex-direction: column; align-items: center;
    text-align: center; padding: 24px 18px;
  }
  .hero-art { width: 188px; height: 188px; border-radius: 18px; object-fit: cover; box-shadow: 0 18px 36px rgba(0,0,0,0.45); }
  .hero-art.placeholder { display: grid; place-items: center; background: rgba(118,118,128,0.22); }
  .hero-title {
    width: 100%; margin: 14px 0 4px; font-family: var(--font-display);
    font-size: 20px; line-height: 1.25; text-align: center;
  }
  .hero-sub { width: 100%; font-size: 13px; color: var(--t2); text-align: center; }
  .hero-time { width: 100%; font-size: 12px; color: var(--t2); margin-top: 8px; font-variant-numeric: tabular-nums; text-align: center; }
  .transport {
    width: 100%; max-width: 268px; margin: 18px auto 0;
    display: grid; grid-template-columns: 40px 44px 58px 44px 40px;
    align-items: center; justify-items: center; justify-content: space-between;
  }
  .t-spacer { width: 40px; height: 40px; }
  .play-big {
    width: 58px; height: 58px; padding: 0; border-radius: 50%; border: 0;
    background: var(--t1); color: var(--bg); display: grid; place-items: center; cursor: pointer;
  }

  .ios-bottom {
    position: fixed; left: 0; right: 0; bottom: 0; z-index: 40;
    padding: 8px 12px calc(10px + env(safe-area-inset-bottom, 0px));
    background: var(--dock); backdrop-filter: blur(26px) saturate(190%);
    border-top: 1px solid var(--hair); display: flex; flex-direction: column; gap: 8px;
  }
  .ios-mini {
    background: var(--elevated); border: 1px solid var(--hair); border-radius: 14px;
    padding: 7px 10px; display: flex; align-items: center; gap: 10px;
  }
  .mini-art { width: 36px; height: 36px; border-radius: 8px; object-fit: cover; flex-shrink: 0; }
  .mini-art.placeholder { display: grid; place-items: center; background: rgba(118,118,128,0.22); }
  .ios-tabs { display: grid; grid-template-columns: repeat(4, 1fr); }
  .ios-tab {
    border: 0; background: transparent; color: var(--t2); font-family: inherit; font-size: 10px; font-weight: 600;
    display: flex; flex-direction: column; align-items: center; gap: 4px; padding: 4px 0;
  }
  .ios-tab.active { color: var(--accent); }

  .ios-toast {
    position: fixed; top: 14px; left: 50%; transform: translateX(-50%); z-index: 70;
    background: var(--elevated); border: 1px solid var(--hair); border-radius: 999px;
    padding: 8px 16px; font-size: 12.5px; font-weight: 600; box-shadow: 0 10px 26px rgba(0,0,0,0.4);
  }

  .modal-bg {
    position: fixed; inset: 0; z-index: 90; background: rgba(0,0,0,0.56);
    backdrop-filter: blur(8px); display: grid; place-items: center; padding: 24px;
  }
  .apple-alert {
    width: 100%; max-width: 300px; background: var(--dock); backdrop-filter: blur(30px);
    border: 1px solid var(--hair); border-radius: 16px; overflow: hidden; text-align: center;
  }
  .alert-body { padding: 18px 16px 14px; }
  .alert-body h3 { margin: 0 0 6px; font-family: var(--font-display); font-size: 16px; }
  .alert-body p { margin: 0; font-size: 13px; color: var(--t2); line-height: 1.4; }
  .alert-actions { display: grid; grid-template-columns: 1fr 1fr; border-top: 1px solid var(--hair); }
  .alert-actions.single { grid-template-columns: 1fr; }
  .alert-actions button {
    border: 0; background: transparent; padding: 12px; font-family: inherit; font-size: 15px; color: var(--blue); font-weight: 500;
  }
  .alert-actions button:not(:last-child) { border-right: 1px solid var(--hair); }
  .alert-actions button.destructive { color: var(--red); font-weight: 600; }

  .ios-pin-lock {
    position: fixed; inset: 0; z-index: 100;
    background: var(--bg); display: grid; place-items: center; padding: 24px;
  }
  .pin-card { width: 100%; max-width: 290px; text-align: center; }
  .pin-dots { display: flex; justify-content: center; gap: 14px; margin-bottom: 18px; }
  .pdot {
    width: 13px; height: 13px; border-radius: 50%;
    border: 1.5px solid var(--t2); background: transparent;
  }
  .pdot.filled { background: var(--t1); border-color: var(--t1); }
  .keypad {
    display: grid; grid-template-columns: repeat(3, 72px); gap: 14px;
    justify-content: center; margin-top: 18px;
  }
  .kp-btn {
    width: 72px; height: 72px; border-radius: 50%; border: 0;
    background: rgba(118,118,128,0.22); color: var(--t1);
    font-family: var(--font-display); font-size: 26px; font-weight: 400; cursor: pointer;
  }
  .kp-btn.ghost { background: transparent; font-size: 20px; color: var(--t2); }
</style>
