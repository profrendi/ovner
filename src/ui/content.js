// Skrip yang disuntikkan Ovner ke setiap halaman (hanya bingkai utama):
// 1. pintasan keyboard browser (Ctrl+T, Ctrl+W, Ctrl+L, Ctrl+Tab, ...), karena saat halaman
//    punya fokus, tombolnya tidak sampai ke jendela Ovner;
// 2. memberi tahu Ovner kelas/id elemen di halaman, supaya pemblokir iklan bisa
//    menyembunyikan kotak iklan yang kosong (aturan kosmetik EasyList, seperti Brave);
// 3. memberi tahu bila alamat berubah tanpa memuat ulang (situs satu halaman / SPA);
// 4. pengaturan halaman: matikan spellcheck, abaikan beforeunload, CSS sendiri, AMP → asli;
// 5. pemilih elemen untuk menyembunyikan bagian halaman (seperti element blocker Brave).
(function () {
  if (window.top !== window || window.__ovner) return;
  Object.defineProperty(window, "__ovner", { value: true });
  const ipc = window.ipc;
  const post = (o) => { try { ipc.postMessage(JSON.stringify(o)); } catch (e) {} };
  const cfg = window.__ovnerCfg || {};

  // Dialog "Tinggalkan situs?" (seperti --disable-beforeunload di ungoogled-chromium).
  if (cfg.noBeforeUnload) {
    const add = EventTarget.prototype.addEventListener;
    EventTarget.prototype.addEventListener = function (type) {
      if (this === window && String(type).toLowerCase() === "beforeunload") return;
      return add.apply(this, arguments);
    };
    try { Object.defineProperty(window, "onbeforeunload", { get() { return null; }, set() {}, configurable: false }); } catch (e) {}
  }

  function onReady(fn) {
    if (document.readyState === "loading") document.addEventListener("DOMContentLoaded", fn, { once: true });
    else fn();
  }
  onReady(() => {
    const html = document.documentElement;
    if (!html) return;
    // Halaman AMP: pindah ke versi asli (canonical) bila beda situs/alamat.
    if (cfg.deAmp && (html.hasAttribute("amp") || html.hasAttribute("\u26a1"))) {
      const c = document.querySelector('link[rel="canonical"]');
      if (c && c.href && c.href !== location.href && /^https?:/.test(c.href)) { location.replace(c.href); return; }
    }
    if (!cfg.spellcheck) html.setAttribute("spellcheck", "false");
    if (cfg.userCss) {
      const st = document.createElement("style");
      st.id = "__ovner_css_sendiri";
      st.textContent = cfg.userCss;
      (document.head || html).appendChild(st);
    }
  });

  // Pemilih elemen: sorot elemen di bawah kursor, klik untuk menyembunyikan, Esc untuk batal.
  function cssEscape(s) { return window.CSS && CSS.escape ? CSS.escape(s) : s.replace(/[^a-zA-Z0-9_-]/g, "\\$&"); }
  function selectorFor(el) {
    const path = [];
    for (let e = el, depth = 0; e && e.nodeType === 1 && e !== document.documentElement && depth < 5; e = e.parentElement, depth++) {
      if (e.id && !/\d{3,}/.test(e.id)) { path.unshift("#" + cssEscape(e.id)); break; }
      let s = e.localName;
      const cls = [...e.classList].filter((c) => !/\d{3,}|^(active|hover|show|open)$/.test(c)).slice(0, 2);
      if (cls.length) s += cls.map((c) => "." + cssEscape(c)).join("");
      const sib = e.parentElement ? [...e.parentElement.children].filter((x) => x.localName === e.localName) : [];
      if (sib.length > 1 && !cls.length) s += ":nth-of-type(" + (sib.indexOf(e) + 1) + ")";
      path.unshift(s);
    }
    return path.join(" > ");
  }
  window.__ovnerPick = function () {
    if (window.__ovnerPicking) return;
    window.__ovnerPicking = true;
    const box = document.createElement("div");
    box.style.cssText = "position:fixed;z-index:2147483647;pointer-events:none;background:rgba(232,115,12,.25);outline:2px solid #e8730c;transition:all .05s";
    const tip = document.createElement("div");
    tip.style.cssText = "position:fixed;z-index:2147483647;left:50%;top:10px;transform:translateX(-50%);background:#1d2228;color:#fff;font:13px system-ui;padding:6px 12px;border-radius:6px;pointer-events:none";
    tip.textContent = (navigator.language || "").startsWith("id") ? "Klik bagian yang ingin disembunyikan · Esc untuk batal" : "Click the part to hide · Esc to cancel";
    document.documentElement.append(box, tip);
    let cur = null;
    function move(e) {
      const el = document.elementFromPoint(e.clientX, e.clientY);
      if (!el || el === box || el === tip) return;
      cur = el;
      const r = el.getBoundingClientRect();
      Object.assign(box.style, { left: r.left + "px", top: r.top + "px", width: r.width + "px", height: r.height + "px" });
    }
    function stop() {
      window.__ovnerPicking = false;
      removeEventListener("mousemove", move, true);
      removeEventListener("click", click, true);
      removeEventListener("keydown", key, true);
      box.remove(); tip.remove();
    }
    function click(e) {
      e.preventDefault(); e.stopPropagation();
      if (cur) {
        const sel = selectorFor(cur);
        cur.style.setProperty("display", "none", "important");
        post({ t: "pick", sel });
      }
      stop();
    }
    function key(e) { if (e.key === "Escape") { e.preventDefault(); stop(); } }
    addEventListener("mousemove", move, true);
    addEventListener("click", click, true);
    addEventListener("keydown", key, true);
  };

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
