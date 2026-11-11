export function fmtBytes(bytes) {
  if (!bytes || bytes <= 0) return '0 B';
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(0)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

export function fmtTime(secs) {
  const s = Math.max(0, Math.floor(secs || 0));
  return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, '0')}`;
}

const TOKEN_KEY = 'ytm_session_token';

function getStoredToken() {
  try {
    return localStorage.getItem(TOKEN_KEY) || '';
  } catch {
    return '';
  }
}

function storeToken(tok) {
  try {
    if (tok) localStorage.setItem(TOKEN_KEY, tok);
    else localStorage.removeItem(TOKEN_KEY);
  } catch {}
}

async function api(path, opts = {}) {
  const tok = getStoredToken();
  const res = await fetch(path, {
    credentials: 'same-origin',
    headers: {
      'Content-Type': 'application/json',
      ...(tok ? { Authorization: `Bearer ${tok}` } : {}),
      ...(opts.headers || {})
    },
    ...opts
  });
  if (!res.ok) {
    const txt = await res.text().catch(() => '');
    const err = new Error(txt || `HTTP ${res.status}`);
    err.status = res.status;
    throw err;
  }
  const ct = res.headers.get('content-type') || '';
  return ct.includes('application/json') ? res.json() : res.text();
}

export function createDeckStore() {
  let status = $state({
    video_id: '',
    title: '',
    artist: '',
    album: '',
    playing: false,
    loop_track: false,
    elapsed_secs: 0,
    duration_secs: 0,
    elapsed_str: '0:00',
    duration_str: '0:00',
    theme: 'dark',
    battery: 'AC',
    offline_count: 0,
    offline_bytes: 0,
    orphan_count: 0,
    orphan_bytes: 0,
    fav_count: 0,
    server_url: '',
    queue: [],
    lyrics: []
  });

  let playlists = $state([]);
  let activePlaylistId = $state(null);
  let activeTab = $state('playlists'); // playlists | downloads | orphans | favorites | history
  let tracks = $state([]);
  let searchFilter = $state('');

  let importUrl = $state('');
  let importPreview = $state(null);
  let importBusy = $state(false);

  let selectedIds = $state(new Set());
  let serverOnline = $state(true);
  let authChecking = $state(true);
  let authRequired = $state(false);
  let authError = $state('');
  let toastText = $state('');

  let deferredInstallPrompt = $state(null);
  let isStandalone = $state(
    typeof window !== 'undefined' &&
      (window.matchMedia?.('(display-mode: standalone)').matches || window.navigator?.standalone === true)
  );
  let isIOS = $state(
    typeof navigator !== 'undefined' && /iPhone|iPad|iPod/i.test(navigator.userAgent || '')
  );

  let toastTimer = null;
  function toast(msg) {
    toastText = msg;
    clearTimeout(toastTimer);
    toastTimer = setTimeout(() => {
      toastText = '';
    }, 3200);
  }

  async function checkAuth() {
    authChecking = true;
    try {
      const a = await api('/api/auth/status');
      authRequired = Boolean(a.pin_required && !a.authenticated);
      serverOnline = true;
    } catch {
      serverOnline = false;
    } finally {
      authChecking = false;
    }
  }

  async function verifyPin(pin) {
    authError = '';
    try {
      const res = await api('/api/auth/verify', {
        method: 'POST',
        body: JSON.stringify({ pin: String(pin).trim() })
      });
      if (res?.token) storeToken(res.token);
      authRequired = false;
      await refreshStatus();
      await refreshPlaylists();
      return true;
    } catch (e) {
      let msg = 'Incorrect 6-digit PIN';
      try {
        const parsed = JSON.parse(e.message);
        if (parsed?.error) msg = parsed.error;
      } catch {}
      authError = msg;
      return false;
    }
  }

  async function refreshStatus() {
    if (authRequired) return;
    try {
      const s = await api('/api/status');
      status = s;
      serverOnline = true;
    } catch (e) {
      if (e?.status === 401) {
        authRequired = true;
      } else {
        serverOnline = false;
      }
    }
  }

  async function refreshPlaylists() {
    try {
      const list = await api('/api/playlists');
      playlists = list;
      if (list.length > 0 && !list.some((p) => p.id === activePlaylistId)) {
        activePlaylistId = list[0].id;
      }
      if (activeTab === 'playlists') {
        await loadTracks();
      }
    } catch {}
  }

  async function loadTracks() {
    selectedIds = new Set();
    try {
      if (activeTab === 'playlists') {
        if (activePlaylistId == null) {
          tracks = [];
          return;
        }
        tracks = await api(`/api/playlists/${activePlaylistId}/tracks`);
      } else {
        tracks = await api(`/api/library?tab=${encodeURIComponent(activeTab)}`);
      }
    } catch {}
  }

  async function selectTab(tab) {
    activeTab = tab;
    await loadTracks();
  }

  async function selectPlaylist(id) {
    activePlaylistId = id;
    activeTab = 'playlists';
    await loadTracks();
  }

  async function cmd(action, extra = '') {
    try {
      const tok = getStoredToken();
      const res = await fetch(`/api/cmd?action=${encodeURIComponent(action)}${extra}`, {
        method: 'POST',
        credentials: 'same-origin',
        headers: tok ? { Authorization: `Bearer ${tok}` } : {}
      });
      if (res.status === 401) {
        authRequired = true;
        return;
      }
      setTimeout(refreshStatus, 140);
    } catch {
      toast('Handheld unreachable');
    }
  }

  async function createPlaylist(name) {
    const clean = name.trim();
    if (!clean) return;
    const res = await api('/api/playlists', {
      method: 'POST',
      body: JSON.stringify({ name: clean })
    });
    await refreshPlaylists();
    if (res?.id) {
      await selectPlaylist(res.id);
    }
    toast(`Created "${clean}"`);
  }

  async function renamePlaylist(id, name) {
    const clean = name.trim();
    if (!clean) return;
    await api(`/api/playlists/${id}`, {
      method: 'PATCH',
      body: JSON.stringify({ name: clean })
    });
    await refreshPlaylists();
    toast(`Renamed to "${clean}"`);
  }

  async function deletePlaylist(id, deleteFiles = false) {
    const res = await api(`/api/playlists/${id}?delete_files=${deleteFiles ? 'true' : 'false'}`, {
      method: 'DELETE'
    });
    if (activePlaylistId === id) activePlaylistId = null;
    await refreshPlaylists();
    await refreshStatus();
    toast(
      deleteFiles && res?.freed_bytes
        ? `Deleted playlist & ${fmtBytes(res.freed_bytes)} offline audio`
        : 'Deleted playlist'
    );
  }

  async function removeFromPlaylist(playlistId, videoId) {
    await api(`/api/playlists/${playlistId}/tracks/${encodeURIComponent(videoId)}`, {
      method: 'DELETE'
    });
    await refreshPlaylists();
    toast('Removed from playlist');
  }

  async function deleteOfflineFile(videoId) {
    const res = await api(`/api/files/${encodeURIComponent(videoId)}`, { method: 'DELETE' });
    await loadTracks();
    await refreshPlaylists();
    await refreshStatus();
    toast(`Deleted file (${fmtBytes(res?.freed_bytes || 0)})`);
  }

  async function bulkDeleteFiles(videoIds) {
    const ids = [...videoIds];
    if (!ids.length) return;
    const res = await api('/api/files/bulk-delete', {
      method: 'POST',
      body: JSON.stringify({ video_ids: ids })
    });
    selectedIds = new Set();
    await loadTracks();
    await refreshPlaylists();
    await refreshStatus();
    toast(`Deleted ${res?.removed || ids.length} files (${fmtBytes(res?.freed_bytes || 0)})`);
  }

  async function cleanOrphans() {
    const res = await api('/api/files/clean-orphans', { method: 'POST' });
    await loadTracks();
    await refreshPlaylists();
    await refreshStatus();
    toast(`Cleaned ${res?.removed || 0} orphaned files (${fmtBytes(res?.freed_bytes || 0)})`);
  }

  async function queueDownload(videoId) {
    await api(`/api/files/${encodeURIComponent(videoId)}/download`, { method: 'POST' });
    toast('Queued offline download on RG35XX H');
  }

  async function playTrack(track) {
    await api('/api/play', {
      method: 'POST',
      body: JSON.stringify({ track })
    });
    setTimeout(refreshStatus, 180);
  }

  async function playPlaylistOnHandheld(playlistId) {
    await api(`/api/playlists/${playlistId}/play`, { method: 'POST' });
    setTimeout(refreshStatus, 200);
    toast('Playing playlist on RG35XX H');
  }

  async function previewImport(url = importUrl) {
    const clean = url.trim();
    if (!clean) return;
    importBusy = true;
    try {
      importPreview = await api('/api/import/preview', {
        method: 'POST',
        body: JSON.stringify({ url: clean })
      });
    } catch (e) {
      toast(`Import preview failed: ${e.message}`);
    } finally {
      importBusy = false;
    }
  }

  async function commitImport(download = false, customName = null) {
    if (!importPreview) return;
    importBusy = true;
    try {
      const name = (customName || importPreview.title || 'Imported Playlist').trim();
      const res = await api('/api/import/commit', {
        method: 'POST',
        body: JSON.stringify({
          name,
          tracks: importPreview.tracks,
          download
        })
      });
      importUrl = '';
      importPreview = null;
      await refreshPlaylists();
      if (res?.playlist_id) {
        await selectPlaylist(res.playlist_id);
      }
      toast(
        download
          ? `Imported "${name}" & queued ${res?.track_count || 0} downloads`
          : `Imported "${name}" (${res?.track_count || 0} tracks)`
      );
    } catch (e) {
      toast(`Import failed: ${e.message}`);
    } finally {
      importBusy = false;
    }
  }

  function toggleSelected(videoId) {
    const next = new Set(selectedIds);
    if (next.has(videoId)) next.delete(videoId);
    else next.add(videoId);
    selectedIds = next;
  }

  function selectAllVisible(visibleTracks) {
    if (selectedIds.size === visibleTracks.length && visibleTracks.length > 0) {
      selectedIds = new Set();
    } else {
      selectedIds = new Set(visibleTracks.map((t) => t.video_id));
    }
  }

  async function stopServer() {
    try {
      const tok = getStoredToken();
      await fetch('/api/cmd?action=server_off', {
        method: 'POST',
        credentials: 'same-origin',
        headers: tok ? { Authorization: `Bearer ${tok}` } : {}
      });
    } catch {}
    serverOnline = false;
    toast('Local web server turned off (re-enable in handheld Settings)');
  }

  async function downloadScreenshot() {
    try {
      const tok = getStoredToken();
      const res = await fetch('/api/screenshot.png', {
        credentials: 'same-origin',
        headers: tok ? { Authorization: `Bearer ${tok}` } : {}
      });
      if (res.status === 401) {
        authRequired = true;
        return;
      }
      if (!res.ok) throw new Error('Screenshot unavailable');
      const blob = await res.blob();
      const url = URL.createObjectURL(blob);
      const a = document.createElement('a');
      const stamp = new Date().toISOString().replace(/[:.]/g, '-').slice(0, 19);
      a.href = url;
      a.download = `rusttune-${stamp}.png`;
      document.body.appendChild(a);
      a.click();
      a.remove();
      URL.revokeObjectURL(url);
      toast('Saved 640x480 handheld screenshot');
    } catch (e) {
      toast(`Screenshot failed: ${e.message}`);
    }
  }

  function init() {
    checkAuth().then(() => {
      if (!authRequired) {
        refreshStatus();
        refreshPlaylists();
      }
    });
    const timer = setInterval(refreshStatus, 1400);
    const onPrompt = (e) => {
      e.preventDefault();
      deferredInstallPrompt = e;
    };
    window.addEventListener('beforeinstallprompt', onPrompt);
    return () => {
      clearInterval(timer);
      window.removeEventListener('beforeinstallprompt', onPrompt);
    };
  }

  async function triggerPwaInstall() {
    if (!deferredInstallPrompt) return false;
    deferredInstallPrompt.prompt();
    const choice = await deferredInstallPrompt.userChoice;
    deferredInstallPrompt = null;
    return choice?.outcome === 'accepted';
  }

  return {
    get status() { return status; },
    get playlists() { return playlists; },
    get activePlaylistId() { return activePlaylistId; },
    get activeTab() { return activeTab; },
    get tracks() { return tracks; },
    get searchFilter() { return searchFilter; },
    set searchFilter(v) { searchFilter = v; },
    get importUrl() { return importUrl; },
    set importUrl(v) { importUrl = v; },
    get importPreview() { return importPreview; },
    set importPreview(v) { importPreview = v; },
    get importBusy() { return importBusy; },
    get selectedIds() { return selectedIds; },
    get serverOnline() { return serverOnline; },
    get authChecking() { return authChecking; },
    get authRequired() { return authRequired; },
    get authError() { return authError; },
    get toastText() { return toastText; },
    get deferredInstallPrompt() { return deferredInstallPrompt; },
    get isStandalone() { return isStandalone; },
    get isIOS() { return isIOS; },
    init,
    verifyPin,
    toast,
    selectTab,
    selectPlaylist,
    refreshPlaylists,
    loadTracks,
    cmd,
    createPlaylist,
    renamePlaylist,
    deletePlaylist,
    removeFromPlaylist,
    deleteOfflineFile,
    bulkDeleteFiles,
    cleanOrphans,
    queueDownload,
    playTrack,
    playPlaylistOnHandheld,
    previewImport,
    commitImport,
    toggleSelected,
    selectAllVisible,
    stopServer,
    downloadScreenshot,
    triggerPwaInstall
  };
}
