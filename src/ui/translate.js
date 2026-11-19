// Terjemahan halaman Ovner (disuntikkan hanya saat pengguna menekan Terjemahkan).
// Teks diambil per simpul teks (susunan halaman tidak berubah), yang terlihat di layar lebih
// dulu, dikirim ke Ovner per kelompok (≤ 50 simpul / ±4000 huruf), lalu diganti dengan
// terjemahannya. Teks asli disimpan supaya "Tampilkan asli" langsung mengembalikannya.
// Konten baru (gulir tak berujung, SPA) ikut diterjemahkan selama terjemahan aktif.
(function () {
  if (window.__ovnerTr) return;
  const post = (o) => {
    try {
      if (window.__ovnerBridge) window.__ovnerBridge(o);
      else window.chrome.webview.postMessage(JSON.stringify(o));
    } catch (e) {}
  };
  const SKIP = /^(SCRIPT|STYLE|NOSCRIPT|CODE|PRE|TEXTAREA|INPUT|SELECT|OPTION|SVG|MATH|KBD|SAMP|VAR|TEMPLATE|IFRAME|CANVAS|OVNER-VIDEO-BUTTON)$/;
  const T = {
    on: false, n: 0, to: "", from: "", device: null,
    orig: new Map(),       // simpul → teks asli
    queue: [], inflight: new Map(), seq: 0, done: 0, total: 0, mo: null, timer: 0,
  };
  function skipEl(el) {
    for (let e = el; e && e !== document.documentElement; e = e.parentElement) {
      if (SKIP.test(e.tagName)) return true;
      if (e.isContentEditable) return true;
      if (e.getAttribute && (e.getAttribute("translate") === "no" || e.classList.contains("notranslate"))) return true;
    }
    return false;
  }
  function collect(root) {
    const out = [];
    if (!root) return out;
    const w = document.createTreeWalker(root, NodeFilter.SHOW_TEXT, {
      acceptNode(n) {
        const v = n.nodeValue;
        if (!v || v.trim().length < 2 || !/\p{L}/u.test(v)) return NodeFilter.FILTER_REJECT;
        if (T.orig.has(n)) return NodeFilter.FILTER_REJECT;
        return skipEl(n.parentElement) ? NodeFilter.FILTER_REJECT : NodeFilter.FILTER_ACCEPT;
      },
    });
    for (let n = w.nextNode(); n && out.length < 20000; n = w.nextNode()) out.push(n);
    return out;
  }
  function visible(n) {
    const p = n.parentElement; if (!p) return false;
    const r = p.getBoundingClientRect();
    return r.bottom > -200 && r.top < innerHeight + 400 && r.width > 0;
  }
  function enqueue(nodes) {
    const a = [], b = [];
    for (const n of nodes) (visible(n) ? a : b).push(n);
    T.queue.push(...a, ...b);
    T.total += nodes.length;
  }
  function pump() {
    if (!T.on) return;
    while (T.inflight.size < 3 && T.queue.length) {
      const batch = [], texts = [];
      let chars = 0;
      while (T.queue.length && batch.length < 50 && chars < 4000) {
        const n = T.queue.shift();
        if (!n.isConnected || T.orig.has(n)) { T.done++; continue; }
        const v = n.nodeValue.trim();
        batch.push(n); texts.push(v.slice(0, 4000)); chars += v.length;
      }
      if (!batch.length) break;
      const b = ++T.seq;
      T.inflight.set(b, batch);
      if (T.device) {
        Promise.all(texts.map((t) => T.device.translate(t))).then((r) => T.apply(T.n, b, r, ""), () => { T.device = null; T.inflight.delete(b); T.queue.unshift(...batch); pump(); });
      } else {
        post({ t: "tr-req", n: T.n, b, texts, to: T.to, from: T.from });
      }
    }
    progress();
  }
  let lastP = 0;
  function progress(force) {
    const now = Date.now();
    if (!force && now - lastP < 400) return;
    lastP = now;
    post({ t: "tr-progress", n: T.n, done: T.done, total: T.total, busy: T.inflight.size > 0 || T.queue.length > 0 });
  }
  T.apply = function (n, b, arr, detected) {
    if (n !== T.n) return;
    const batch = T.inflight.get(b);
    if (!batch) return;
    T.inflight.delete(b);
    if (detected && !T.from) { T.from = detected; post({ t: "tr-lang", n: T.n, lang: detected }); }
    batch.forEach((node, i) => {
      T.done++;
      const tr = arr[i];
      if (!T.on || !node.isConnected || typeof tr !== "string" || !tr) return;
      const v = node.nodeValue;
      const lead = v.match(/^\s*/)[0], trail = v.match(/\s*$/)[0];
      T.orig.set(node, v);
      node.nodeValue = lead + tr + trail;
    });
    pump();
    if (!T.inflight.size && !T.queue.length) progress(true);
  };
  T.fail = function (n, b, msg) {
    if (n !== T.n) return;
    const batch = T.inflight.get(b) || [];
    T.inflight.delete(b);
    T.done += batch.length;
    post({ t: "tr-progress", n: T.n, done: T.done, total: T.total, busy: false, error: String(msg || "") });
  };
  function observe() {
    if (T.mo) return;
    const added = new Set();
    T.mo = new MutationObserver((list) => {
      if (!T.on) return;
      for (const m of list) for (const nd of m.addedNodes) added.add(nd);
      clearTimeout(T.timer);
      T.timer = setTimeout(() => {
        if (!T.on) return;
        const fresh = [];
        for (const nd of added) {
          if (!nd.isConnected) continue;
          if (nd.nodeType === 3) {
            const v = nd.nodeValue;
            if (v && v.trim().length > 1 && /\p{L}/u.test(v) && !T.orig.has(nd) && !skipEl(nd.parentElement)) fresh.push(nd);
          } else if (nd.nodeType === 1 && !skipEl(nd)) fresh.push(...collect(nd));
        }
        added.clear();
        if (fresh.length) { enqueue(fresh); pump(); }
      }, 700);
    });
    T.mo.observe(document.body || document.documentElement, { childList: true, subtree: true });
  }
  T.start = async function (n, to, from) {
    T.restore(true);
    T.n = n; T.to = to; T.from = from || ""; T.on = true; T.done = 0; T.total = 0; T.queue = []; T.inflight.clear();
    T.device = null;
    // Penerjemah offline bawaan Chromium (Translator API) bila tersedia dan bahasanya diketahui.
    try {
      if (T.from && self.Translator && Translator.availability) {
        const a = await Translator.availability({ sourceLanguage: T.from.slice(0, 2), targetLanguage: to.slice(0, 2) });
        if (a === "available") { T.device = await Translator.create({ sourceLanguage: T.from.slice(0, 2), targetLanguage: to.slice(0, 2) }); post({ t: "tr-device", n }); }
      }
    } catch (e) { T.device = null; }
    enqueue(collect(document.body));
    if (document.title) {
      // Judul halaman ikut diterjemahkan lewat simpul teks <title>.
      const tt = document.querySelector("title");
      if (tt && tt.firstChild && !T.orig.has(tt.firstChild)) { T.queue.unshift(tt.firstChild); T.total++; }
    }
    observe();
    pump();
    progress(true);
  };
  T.restore = function (quiet) {
    T.on = false;
    clearTimeout(T.timer);
    if (T.mo) { T.mo.disconnect(); T.mo = null; }
    for (const [node, v] of T.orig) { try { node.nodeValue = v; } catch (e) {} }
    T.orig.clear(); T.queue = []; T.inflight.clear();
    if (!quiet) post({ t: "tr-progress", n: T.n, done: 0, total: 0, busy: false, restored: true });
  };
  // Terjemahan teks yang dipilih (klik kanan): hasilnya ditampilkan di panel terjemah toolbar.
  Object.defineProperty(window, "__ovnerTr", { value: T, configurable: false });
})();
