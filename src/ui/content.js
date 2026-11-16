// Skrip yang disuntikkan Ovner ke setiap halaman (hanya bingkai utama):
// 1. pintasan keyboard browser (Ctrl+T, Ctrl+W, Ctrl+L, Ctrl+Tab, ...), karena saat halaman
//    punya fokus, tombolnya tidak sampai ke jendela Ovner;
// 2. memberi tahu Ovner kelas/id elemen di halaman, supaya pemblokir iklan bisa
//    menyembunyikan kotak iklan yang kosong (aturan kosmetik EasyList, seperti Brave);
// 3. memberi tahu bila alamat berubah tanpa memuat ulang (situs satu halaman / SPA).
(function () {
  if (window.top !== window || window.__ovner) return;
  Object.defineProperty(window, "__ovner", { value: true });
  const ipc = window.ipc;
  const post = (o) => { try { ipc.postMessage(JSON.stringify(o)); } catch (e) {} };

  window.addEventListener("keydown", (e) => {
    const k = (e.key || "").toLowerCase();
    let c = null;
    if (e.ctrlKey && !e.altKey && !e.metaKey) {
      if (k === "t") c = e.shiftKey ? "reopen" : "newtab";
      else if (k === "n") c = e.shiftKey ? "private" : "newtab";
      else if (k === "w" || k === "f4") c = "close";
      else if (k === "l") c = "address";
      else if (k === "f") c = "findbar";
      else if (k === "d") c = "bookmark";
      else if (k === "h") c = "history";
      else if (k === "j") c = "downloads";
      else if (k === "p") c = "print";
      else if (k === "tab") c = e.shiftKey ? "prevtab" : "nexttab";
      else if (k === "pagedown") c = "nexttab";
      else if (k === "pageup") c = "prevtab";
      else if (/^[1-9]$/.test(k)) c = "tab" + k;
    } else if (e.altKey && !e.ctrlKey && k === "d") c = "address";
    else if (e.altKey && !e.ctrlKey && k === "home") c = "home";
    else if (k === "f6") c = "address";
    else if (k === "escape" && e.shiftKey) c = "tasks";
    if (c) {
      e.preventDefault();
      e.stopImmediatePropagation();
      post({ t: "key", k: c });
    }
  }, true);

  // Alamat berubah lewat history API (YouTube, Gmail, dsb.).
  const notifyUrl = () => post({ t: "url" });
  for (const fn of ["pushState", "replaceState"]) {
    const orig = history[fn];
    history[fn] = function () {
      const r = orig.apply(this, arguments);
      notifyUrl();
      return r;
    };
  }
  window.addEventListener("popstate", notifyUrl);
  window.addEventListener("hashchange", notifyUrl);

  // Kosmetik: kirim kelas/id baru, Ovner membalas dengan CSS penyembunyi yang cocok.
  post({ t: "page" });
  const seenC = new Set(), seenI = new Set();
  let qc = [], qi = [], timer = 0;
  function add(el) {
    if (el.id && !seenI.has(el.id)) { seenI.add(el.id); qi.push(el.id); }
    const cl = el.classList;
    if (cl) for (const c of cl) if (!seenC.has(c)) { seenC.add(c); qc.push(c); }
  }
  function scan(n) {
    if (!n || n.nodeType !== 1) return;
    add(n);
    for (const el of n.querySelectorAll("[id],[class]")) add(el);
  }
  function flush() {
    timer = 0;
    while (qc.length || qi.length) {
      post({ t: "cos", c: qc.splice(0, 3000), i: qi.splice(0, 3000) });
    }
  }
  function later() { if (!timer) timer = setTimeout(flush, 250); }
  function start() {
    scan(document.documentElement);
    flush();
    new MutationObserver((list) => {
      for (const m of list) {
        if (m.type === "attributes") add(m.target);
        else for (const n of m.addedNodes) scan(n);
      }
      if (qc.length || qi.length) later();
    }).observe(document.documentElement, { childList: true, subtree: true, attributes: true, attributeFilter: ["class", "id"] });
  }
  if (document.readyState === "loading") document.addEventListener("DOMContentLoaded", start, { once: true });
  else start();
})();
