import { mount } from 'svelte';

if ('serviceWorker' in navigator) {
  navigator.serviceWorker.register('/sw.js').catch(() => {});
}

// Mirror annie-desk device detection: mount MobileApp ONLY on phones,
// keeping the full desktop Command Workbench on laptops/desktops
// (unless ?view=phone or ?view=desktop is explicitly passed).
const params = new URLSearchParams(window.location.search);
const forcedView = params.get('view');
const isMobileUA = /Mobi|Android|iPhone|iPod/i.test(navigator.userAgent || '');
const isPhone = forcedView === 'phone' || (forcedView !== 'desktop' && isMobileUA && window.innerWidth <= 768);

const target = document.getElementById('app');
const loadApp = isPhone ? () => import('./MobileApp.svelte') : () => import('./App.svelte');

loadApp().then((mod) => {
  mount(mod.default, { target });
});
