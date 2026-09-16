(async () => {
  const wait = ms => new Promise(resolve => setTimeout(resolve, ms));
  for (let attempt = 0; attempt < 40; attempt++) {
    if (document.querySelector('#replay-menu-status-title')?.textContent === 'Ready to save') break;
    await wait(100);
  }
  const panel = document.querySelector('.replay-menu-panel');
  const rect = panel?.getBoundingClientRect();
  const value = {
    url: location.href,
    shellAbsent: !document.querySelector('.sidebar,.topbar,.brand,.app-shell'),
    rootTransparent: getComputedStyle(document.documentElement).backgroundColor === 'rgba(0, 0, 0, 0)',
    bodyTransparent: getComputedStyle(document.body).backgroundColor === 'rgba(0, 0, 0, 0)',
    ready: document.querySelector('#replay-menu-status-title')?.textContent === 'Ready to save',
    fits: !!rect && rect.left >= 0 && rect.right <= innerWidth && rect.top >= 0 && rect.bottom <= innerHeight,
    noOverflow: document.documentElement.scrollHeight <= innerHeight,
    nativeSize: innerWidth === 760 && innerHeight === 520,
  };
  value.passed = location.pathname.endsWith('/replay-menu.html') && Object.entries(value).filter(([key]) => key !== 'url').every(([, v]) => v === true);
  await window.__TAURI_INTERNALS__.invoke('probe_report', { value });
})();
