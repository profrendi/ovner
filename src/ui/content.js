// Pembuat SVG tanpa innerHTML (aman untuk situs dengan Trusted Types).
function __ovnerSvg(viewBox, kids, attrs) {
  const NS = "http://www.w3.org/2000/svg";
  const svg = document.createElementNS(NS, "svg");
  svg.setAttribute("viewBox", viewBox);
  for (const k in attrs || {}) svg.setAttribute(k, String(attrs[k]));
  for (const [tag, a] of kids) { const e = document.createElementNS(NS, tag); for (const k in a) e.setAttribute(k, String(a[k])); svg.append(e); }
  return svg;
}
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
  // Saluran ke Ovner diambil sekarang: privacy.js lalu menyembunyikan chrome.webview/ipc
  // dari skrip situs.
  const cw = window.chrome && window.chrome.webview;
  const send = cw && cw.postMessage ? cw.postMessage.bind(cw) : null;
  const ipc = window.ipc;
  const post = (o) => { try { const s = JSON.stringify(o); if (send) send(s); else ipc.postMessage(s); } catch (e) {} };
  const cfg = window.__ovnerCfg || {};
  // Saluran untuk skrip yang disuntikkan Ovner belakangan (terjemahan, baca keras): chrome.webview
  // sudah disembunyikan privacy.js saat itu. Hanya pesan terjemahan/baca keras yang diteruskan.
  try { Object.defineProperty(window, "__ovnerBridge", { value: (o) => { if (o && /^(tr-(req|progress|lang|device)|read-text)$/.test(o.t)) post(o); } }); } catch (e) {}

  // Dekoder video GPU komputer ini (untuk mode codec YouTube "Otomatis"): diperiksa sekali per
  // sesi di halaman https pertama. Fungsi aslinya diambil sekarang, sebelum privacy.js
  // menimpanya di YouTube. WebCodecs "prefer-hardware" = hanya dekoder GPU; bila tidak ada,
  // MediaCapabilities.powerEfficient dipakai.
  if (cfg.yt && cfg.yt.probe && window.isSecureContext && /^https?:$/.test(location.protocol)) {
    const VD = window.VideoDecoder, ics = VD && VD.isConfigSupported;
    const mc = navigator.mediaCapabilities, di = mc && mc.decodingInfo;
    const probe = async () => {
      const C = { h264: ["avc1.640028", "mp4"], vp9: ["vp09.00.40.08", "webm"], av1: ["av01.0.08M.08", "mp4"] };
      const out = { t: "codec-hw" };
      for (const [k, [codec, box]] of Object.entries(C)) {
        let hw = null;
        try {
          if (ics) hw = !!(await ics.call(VD, { codec, codedWidth: 1920, codedHeight: 1080, hardwareAcceleration: "prefer-hardware" })).supported;
        } catch (e) {}
        if (hw === null && di) {
          try {
            const r = await di.call(mc, { type: "media-source", video: { contentType: `video/${box}; codecs="${codec}"`, width: 1920, height: 1080, bitrate: 4000000, framerate: 30 } });
            hw = !!(r.supported && r.powerEfficient);
          } catch (e) {}
        }
        out[k] = hw;
      }
      post(out);
    };
    const later = () => setTimeout(() => { probe().catch(() => {}); }, 1500);
    if (document.readyState === "complete") later(); else addEventListener("load", later, { once: true });
  }

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
    if (cfg.pageFont) {
      const st = document.createElement("style");
      st.id = "__ovner_font";
      st.textContent = cfg.pageFont;
      (document.head || html).appendChild(st);
    }
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
      else if (k === "n") c = e.shiftKey ? "private" : "newwindow";
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
    // Paksa tempel (seperti Allow Copy+): Alt+Shift+V di kolom yang melarang tempel.
    if (e.altKey && e.shiftKey && !e.ctrlKey && k === "v") {
      e.preventDefault();
      e.stopImmediatePropagation();
      pasteTarget = document.activeElement;
      post({ t: "paste" });
      return;
    }
    if (c) {
      e.preventDefault();
      e.stopImmediatePropagation();
      post({ t: "key", k: c });
    }
  }, true);

  // Ctrl+U = lihat sumber halaman, Ctrl+Shift+U = baca keras (seperti Edge). Tahap bubble dan
  // hanya bila halaman tidak memakainya sendiri (mis. garis bawah di Google Docs).
  addEventListener("keydown", (e) => {
    if (e.defaultPrevented || !e.ctrlKey || e.altKey || e.metaKey || (e.key || "").toLowerCase() !== "u") return;
    e.preventDefault();
    post({ t: "key", k: e.shiftKey ? "read" : "source" });
  });

  // Bahasa halaman untuk ikon terjemah: atribut lang, dikoreksi dengan tebakan isi (banyak blog
  // Indonesia bawaan Blogger memasang lang="en").
  if (window.top === window && /^https?:$/.test(location.protocol)) {
    const guess = () => {
      try {
        const attr = (document.documentElement.lang || "").trim();
        const text = ((document.body && document.body.innerText) || "").slice(0, 4000).toLowerCase();
        const words = text.match(/\p{L}+/gu) || [];
        let idn = 0, eng = 0;
        const ID = new Set(["yang", "dan", "di", "dengan", "untuk", "ini", "dari", "dalam", "tidak", "akan", "pada", "adalah", "itu", "juga", "atau", "bisa", "kami", "anda", "ke", "oleh"]);
        const EN = new Set(["the", "and", "of", "to", "is", "in", "that", "for", "with", "this", "are", "on", "you", "it", "as", "be", "was", "by", "or", "from"]);
        for (const w of words) { if (ID.has(w)) idn++; else if (EN.has(w)) eng++; }
        let l = attr;
        if (words.length > 40) {
          if (idn >= 6 && idn > eng * 1.6) l = /^id|^in/i.test(attr) ? attr : "id";
          else if (eng >= 6 && eng > idn * 1.6 && (!attr || /^id|^in/i.test(attr))) l = "en";
        }
        post({ t: "page-lang", l: l.slice(0, 12) });
      } catch (e) {}
    };
    const later = () => (window.requestIdleCallback ? requestIdleCallback(guess, { timeout: 3000 }) : setTimeout(guess, 1500));
    if (document.readyState === "complete") later(); else addEventListener("load", later, { once: true });
  }

  // ---- Formulir sedang diisi: tab ini jangan ditidurkan (teks yang diketik tidak hilang). ----
  let dirty = false;
  document.addEventListener("input", (e) => {
    const el = e.target;
    if (dirty || !el) return;
    if (/^(INPUT|TEXTAREA|SELECT)$/.test(el.tagName || "") || el.isContentEditable) {
      if (el.type === "search" || el.type === "password") return;
      dirty = true;
      post({ t: "dirty" });
    }
  }, true);
  document.addEventListener("submit", () => { if (dirty) { dirty = false; post({ t: "clean" }); } }, true);

  // ---- Paksa tempel: teks dimasukkan seolah diketik (tanpa event paste yang bisa ditolak) ----
  let pasteTarget = null;
  Object.defineProperty(window, "__ovnerForcePaste", {
    value: (text) => {
      const el = pasteTarget && pasteTarget.isConnected ? pasteTarget : document.activeElement;
      if (!el || typeof text !== "string") return;
      try { el.focus(); } catch (e) {}
      if (document.execCommand && document.execCommand("insertText", false, text)) return;
      if ("value" in el) {
        const a = el.selectionStart ?? el.value.length, b = el.selectionEnd ?? el.value.length;
        const proto = el instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
        const set = Object.getOwnPropertyDescriptor(proto, "value").set;
        set.call(el, el.value.slice(0, a) + text + el.value.slice(b));
        el.dispatchEvent(new Event("input", { bubbles: true }));
        el.dispatchEvent(new Event("change", { bubbles: true }));
      }
    },
  });

  // ---- Anubis ("Making sure you're not a bot!", dipakai Startpage). Skrip Anubis sendiri
  // dibiarkan bekerja lebih dulu: dua jawaban untuk satu tantangan ditolak server ("double
  // spend" → "Something went wrong during verification"). Bila setelah 10 detik halaman masih
  // di tantangan (skripnya macet/lambat), Ovner memecahkannya secara native (Rust). Bila
  // verifikasi tetap gagal, alamat aslinya dibuka ulang sekali untuk tantangan baru. ----
  onReady(() => {
    const onPass = /\/\.within\.website\/x\/cmd\/anubis\/api\/pass-challenge/.test(location.pathname);
    if (onPass) {
      const redir = new URLSearchParams(location.search).get("redir");
      const text = (document.body && document.body.innerText) || "";
      let tries = 0;
      try { tries = Number(sessionStorage.getItem("__ovnerAnubisRetry") || 0); } catch (e) {}
      if (redir && tries < 2 && /went wrong|verification|double|error|kesalahan/i.test(text)) {
        try { sessionStorage.setItem("__ovnerAnubisRetry", String(tries + 1)); } catch (e) {}
        let target = redir;
        try { target = new URL(redir, location.origin).href; } catch (e) {}
        if (target.startsWith(location.origin)) setTimeout(() => location.replace(target), 400);
      }
      return;
    }
    const el = document.getElementById("anubis_challenge");
    if (!el) { try { sessionStorage.removeItem("__ovnerAnubisRetry"); } catch (e) {} return; }
    let data;
    try { data = JSON.parse(el.textContent); } catch (e) { return; }
    const ch = data && data.challenge, rules = (data && data.rules) || {};
    const random = typeof ch === "string" ? ch : ch && ch.randomData;
    const algo = rules.algorithm || (ch && ch.method) || "fast";
    const diff = Number(rules.difficulty);
    if (!random || !/^(fast|slow)$/.test(algo) || !(diff >= 1 && diff <= 7)) return;
    let prefix = "";
    try { const bp = document.getElementById("anubis_base_prefix"); if (bp) prefix = JSON.parse(bp.textContent) || ""; } catch (e) {}
    const t0 = Date.now();
    const here = location.href;
    Object.defineProperty(window, "__ovnerAnubis", {
      value: (hash, nonce) => {
        if (location.href !== here || !document.getElementById("anubis_challenge")) return;
        const q = new URLSearchParams();
        if (ch && typeof ch === "object" && ch.id) q.set("id", ch.id);
        q.set("response", hash); q.set("nonce", String(nonce)); q.set("redir", location.href);
        q.set("elapsedTime", String(Math.max(1, Date.now() - t0)));
        location.replace(prefix + "/.within.website/x/cmd/anubis/api/pass-challenge?" + q.toString());
      },
    });
    // Startpage: skrip Anubis halaman dimatikan Ovner, jadi langsung dipecahkan native.
    const instant = /(^|\.)startpage\.com$/.test(location.hostname);
    setTimeout(() => {
      if (location.href === here && document.getElementById("anubis_challenge")) post({ t: "anubis", data: random, difficulty: diff });
    }, instant ? 0 : 10000);
  });

  // Alamat berubah lewat history API (YouTube, Gmail, dsb.).
  // ---- Filter kosmetik procedural (:has-text, :upward, :matches-css, :remove, ...) ----
  // Format JSON dari adblock-rust: {selector:[{type, arg}...], action:{type, arg}?}.
  // Dijalankan ulang saat halaman berubah (MutationObserver, dibatasi).
  (function () {
    let rules = [];
    const toRe = (s) => {
      const m = /^\/(.*)\/([a-z]*)$/.exec(s);
      if (m) { try { return new RegExp(m[1], m[2]); } catch (e) { return null; } }
      return null;
    };
    const textTest = (pat) => {
      const re = toRe(pat);
      return re ? (t) => re.test(t) : (t) => t.includes(pat);
    };
    const unq = (s) => String(s).trim().replace(/^"(.*)"$/, "$1");
    function step(els, op) {
      const a = op.arg;
      switch (op.type) {
        case "css-selector": {
          const out = [];
          const sel = a.trim();
          for (const el of els) {
            if (/^[+~]/.test(sel)) {
              const rest = sel.slice(1).trim();
              let s = el.nextElementSibling;
              while (s) { if (s.matches(rest)) { out.push(s); if (sel[0] === "+") break; } else if (sel[0] === "+") break; s = s.nextElementSibling; }
            } else {
              try { out.push(...el.querySelectorAll(":scope " + sel)); } catch (e) {}
            }
          }
          return out;
        }
        case "has-text": { const t = textTest(a); return els.filter((e) => t(e.textContent || "")); }
        case "min-text-length": { const n = parseInt(a, 10) || 0; return els.filter((e) => (e.textContent || "").length >= n); }
        case "matches-path": { const t = textTest(a); return t(location.pathname + location.search) ? els : []; }
        case "matches-attr": {
          const [rawName, ...rest] = a.split("=");
          const name = unq(rawName), val = rest.length ? unq(rest.join("=")) : null;
          const nRe = toRe(name), vRe = val !== null ? toRe(val) : null;
          return els.filter((e) => [...e.attributes].some((at) =>
            (nRe ? nRe.test(at.name) : at.name === name) && (val === null || (vRe ? vRe.test(at.value) : at.value === val))));
        }
        case "matches-css": case "matches-css-before": case "matches-css-after": {
          const pseudo = op.type === "matches-css" ? null : op.type.endsWith("before") ? "::before" : "::after";
          const i = a.indexOf(":");
          const prop = a.slice(0, i).trim(), want = a.slice(i + 1).trim();
          const re = toRe(want);
          return els.filter((e) => { const v = getComputedStyle(e, pseudo).getPropertyValue(prop); return re ? re.test(v) : v === want; });
        }
        case "upward": {
          const n = parseInt(a, 10);
          const out = [];
          for (const e of els) {
            let p = e;
            if (String(n) === a.trim()) { for (let k = 0; k < n && p; k++) p = p.parentElement; }
            else { p = e.parentElement && e.parentElement.closest(a); }
            if (p && p !== document.documentElement) out.push(p);
          }
          return out;
        }
        case "xpath": {
          const out = [];
          for (const e of els) {
            try {
              const r = document.evaluate(a, e, null, XPathResult.ORDERED_NODE_SNAPSHOT_TYPE, null);
              for (let k = 0; k < r.snapshotLength; k++) { const n = r.snapshotItem(k); if (n.nodeType === 1) out.push(n); }
            } catch (e2) {}
          }
          return out;
        }
        default: return [];
      }
    }
    function select(ops) {
      if (!ops.length) return [];
      let els;
      if (ops[0].type === "css-selector") { try { els = [...document.querySelectorAll(ops[0].arg)]; } catch (e) { return []; } ops = ops.slice(1); }
      else els = [...document.querySelectorAll("*")].filter((e) => e !== document.documentElement && e !== document.body);
      for (const op of ops) { els = step(els, op); if (!els.length) break; }
      return [...new Set(els)];
    }
    function apply() {
      for (const r of rules) {
        let els;
        try { els = select(r.selector || []); } catch (e) { continue; }
        const act = r.action || { type: "hide" };
        for (const e of els) {
          if (e.__ovnerDone === r) continue;
          e.__ovnerDone = r;
          if (act.type === "remove") e.remove();
          else if (act.type === "style") e.style.cssText += ";" + act.arg.split(";").map((d) => d.trim() ? d + " !important" : "").join(";");
          else if (act.type === "remove-attr") e.removeAttribute(act.arg);
          else if (act.type === "remove-class") act.arg.split("|").forEach((c) => e.classList.remove(c.trim()));
          else e.style.setProperty("display", "none", "important");
        }
      }
    }
    let pending = false;
    // Halaman tersembunyi (tab latar/jendela diminimalkan): kerjakan jauh lebih jarang.
    const later = () => { if (pending) return; pending = true; setTimeout(() => { pending = false; apply(); }, document.hidden ? 2000 : 150); };
    window.__ovnerProc = (list) => {
      rules = (list || []).filter((r) => r && Array.isArray(r.selector));
      if (!rules.length) return;
      onReady(() => {
        apply();
        new MutationObserver(later).observe(document.documentElement, { childList: true, subtree: true });
      });
    };
  })();

  // ---- Pengelola sandi: isi otomatis dan tawaran simpan (seperti Brave) ----
  (function () {
    let offers = [], asked = false, box = null, lastSent = "";
    // Terlihat = punya kotak di layar (offsetParent null untuk kolom di dalam dialog
    // position:fixed, jadi tidak dipakai), tidak tersembunyi, dan bisa diisi.
    const visible = (e) => {
      if (!e || e.disabled || e.readOnly || e.type === "hidden") return false;
      if (!e.getClientRects().length) return false;
      const cs = getComputedStyle(e);
      return cs.visibility !== "hidden" && cs.display !== "none" && Number(cs.opacity) > 0.05;
    };
    const pwFields = () => [...document.querySelectorAll('input[type="password"]')].filter(visible);
    // Kolom nama pengguna: input teks/email terakhir sebelum kolom sandi.
    function userFieldFor(pw) {
      const scope = pw.form || document;
      const cands = [...scope.querySelectorAll('input:not([type]),input[type="text"],input[type="email"],input[type="tel"],input[type="number"]')].filter(visible);
      // 1. Ditandai situs sebagai nama pengguna.
      const marked = cands.find((c) => /username|email/i.test(c.autocomplete || ""));
      if (marked) return marked;
      // 2. Kolom terakhir sebelum kolom sandi (urutan formulir login biasa).
      let best = null;
      for (const c of cands) if (c.compareDocumentPosition(pw) & Node.DOCUMENT_POSITION_FOLLOWING) best = c;
      return best || cands.find((c) => /user|email|login|akun|nama|nik|npp|nip|id/i.test(c.name + c.id + c.placeholder)) || null;
    }
    const setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value").set;
    let rows = [], sel = -1, anchorEl = null;
    function hide() { if (box) { box.remove(); box = null; } rows = []; sel = -1; anchorEl = null; }
    // Daftar Ovner saja: daftar "Saved info" bawaan WebView2 dimatikan pada kolom ini supaya tidak
    // ada dua kotak bertumpuk.
    function quiet(el) { try { if (el && el.getAttribute("autocomplete") !== "off") el.setAttribute("autocomplete", "off"); } catch (e) {} }
    // Saran yang cocok dengan yang sudah diketik (awalan dulu, lalu yang memuat teksnya).
    function matching(anchor) {
      const q = anchor && anchor.type !== "password" ? anchor.value.trim().toLowerCase() : "";
      if (!q) return offers;
      const pre = [], mid = [];
      for (const o of offers) {
        const u = (o.user || "").toLowerCase();
        if (u.startsWith(q)) pre.push(o); else if (u.includes(q)) mid.push(o);
      }
      return pre.concat(mid);
    }
    function choose(o) { hide(); post({ t: "pw-fill", id: o.id, url: location.href }); }
    function mark(i) {
      sel = i;
      rows.forEach((r, j) => r.classList.toggle("on", j === i));
      if (rows[i]) rows[i].scrollIntoView({ block: "nearest" });
    }
    // Kotak saran dibuat dengan elemen sendiri + shadow DOM + stylesheet buatan (adoptedStyleSheets):
    // tidak terpengaruh CSS situs dan tetap tampil di situs dengan CSP ketat (mis. login Infor),
    // yang memblokir <style> biasa sehingga dulu ikon kunci tampil raksasa di bawah halaman.
    function show(anchor) {
      const list = matching(anchor);
      hide();
      if (!list.length || !anchor) return;
      quiet(anchor);
      anchorEl = anchor;
      const host = document.createElement("ovner-pw");
      const root = host.attachShadow({ mode: "closed" });
      const r = anchor.getBoundingClientRect();
      // Daftar bisa digulir; bila ruang di bawah kolom sempit, tampil di atasnya.
      const below = innerHeight - r.bottom - 8, above = r.top - 8;
      const up = below < 180 && above > below;
      const maxH = Math.max(120, Math.min(320, up ? above : below));
      const rowH = 46, want = Math.min(maxH, list.length * rowH + 10);
      const top = up ? Math.max(4, r.top - 2 - want) : r.bottom + 2;
      const pos = { position: "fixed", "z-index": "2147483647", left: r.left + "px", top: top + "px",
        "min-width": Math.max(220, r.width) + "px", display: "block", margin: "0", padding: "0", border: "0",
        background: "transparent", transform: "none", width: "auto", height: "auto" };
      for (const k in pos) host.style.setProperty(k, pos[k], "important");
      const dark = matchMedia("(prefers-color-scheme: dark)").matches;
      const css = `.b{font:13px "Segoe UI",system-ui,sans-serif;background:${dark ? "#2b2f35" : "#fff"};color:${dark ? "#e8eaed" : "#1d2228"};
          border:1px solid ${dark ? "#454b53" : "#c9cfd6"};border-radius:8px;box-shadow:0 6px 20px #0004;padding:4px;text-align:left;line-height:1.35;
          max-height:${maxH}px;overflow-y:auto;overscroll-behavior:contain}
        .i{display:flex;align-items:center;gap:10px;padding:8px 10px;border-radius:6px;cursor:pointer}
        .i:hover,.i.on{background:${dark ? "#3a4048" : "#e6ecf5"}}
        .k{width:16px;height:16px;flex:none;opacity:.7}
        .u{font-weight:600}.u b{color:${dark ? "#8ab4f8" : "#1a62d6"}}.s{font-size:11px;opacity:.65}`;
      try {
        const sheet = new CSSStyleSheet();
        sheet.replaceSync(css);
        root.adoptedStyleSheets = [sheet];
      } catch (e) {
        const st = document.createElement("style"); st.textContent = css; root.append(st);
      }
      const b = document.createElement("div"); b.className = "b";
      root.append(b);
      const q = anchor.type !== "password" ? anchor.value.trim() : "";
      for (const o of list) {
        const it = document.createElement("div"); it.className = "i";
        // Tanpa innerHTML: situs dengan Trusted Types (mis. YouTube, Google) menolaknya.
        const svgK = __ovnerSvg("0 0 24 24", [["circle", { cx: 8, cy: 15, r: 4 }], ["path", { d: "M11 12l9-9M17 6l3 3" }]], { class: "k", width: 16, height: 16, fill: "none", stroke: "currentColor", "stroke-width": 2 });
        const u = document.createElement("div"); u.className = "u";
        const sdots = document.createElement("div"); sdots.className = "s"; sdots.textContent = "••••••••";
        const wrap = document.createElement("div"); wrap.append(u, sdots);
        it.append(svgK, wrap);
        const name = o.user || "(tanpa nama)";
        const at = q ? name.toLowerCase().indexOf(q.toLowerCase()) : -1;
        if (at >= 0) {
          u.append(name.slice(0, at));
          const m = document.createElement("b"); m.textContent = name.slice(at, at + q.length); u.append(m);
          u.append(name.slice(at + q.length));
        } else u.textContent = name;
        it.addEventListener("mousedown", (e) => { e.preventDefault(); e.stopPropagation(); choose(o); });
        it.__o = o;
        b.append(it);
        rows.push(it);
      }
      document.documentElement.append(host);
      box = host;
      if (q && rows.length) mark(0);
    }
    const managed = (el) => el instanceof HTMLInputElement && (el.type === "password" || pwFields().some((p) => userFieldFor(p) === el));
    // Mengetik di kolom pengguna langsung menyaring daftar.
    document.addEventListener("input", (e) => {
      if (offers.length && managed(e.target) && e.target.type !== "password") show(e.target);
    }, true);
    // Panah atas/bawah, Enter, Esc seperti daftar isi otomatis Chrome.
    document.addEventListener("keydown", (e) => {
      if (!box || e.target !== anchorEl) return;
      if (e.key === "ArrowDown") { e.preventDefault(); e.stopImmediatePropagation(); mark(Math.min(rows.length - 1, sel + 1)); }
      else if (e.key === "ArrowUp") { e.preventDefault(); e.stopImmediatePropagation(); mark(Math.max(0, sel - 1)); }
      else if (e.key === "Enter" && rows[sel]) { e.preventDefault(); e.stopImmediatePropagation(); choose(rows[sel].__o); }
      else if (e.key === "Escape") hide();
    }, true);
    // Isi seperti mengetik: banyak situs (Angular/React) baru menerima nilai setelah event ini.
    function type(el, v) {
      if (!el) return;
      el.focus();
      setter.call(el, v);
      for (const t of ["input", "change"]) el.dispatchEvent(new Event(t, { bubbles: true }));
      el.dispatchEvent(new KeyboardEvent("keyup", { bubbles: true, key: "Unidentified" }));
    }
    window.__ovnerPw = {
      offer(list, auto) {
        offers = list || [];
        if (auto && offers.length === 1) post({ t: "pw-fill", id: offers[0].id, url: location.href });
        const a = document.activeElement;
        for (const p of pwFields()) { quiet(p); quiet(userFieldFor(p)); }
        if (a && a.tagName === "INPUT" && managed(a)) show(a);
      },
      fill(user, pass) {
        // Kolom nama pengguna kadang baru muncul/dirender sesaat setelah kolom sandi: coba lagi
        // sampai 3 detik supaya email/ID tidak perlu diketik sendiri.
        let tries = 0;
        const go = () => {
          const pw = pwFields()[0];
          const u = pw ? userFieldFor(pw) : null;
          if ((!pw || (user && !u)) && ++tries < 15) { setTimeout(go, 200); return; }
          if (u && user && u.value !== user) type(u, user);
          if (pw) type(pw, pass);
          if (u) u.blur();
          hide();
        };
        go();
      },
    };
    // Tanya Ovner sekali bila halaman punya kolom sandi.
    function check() {
      if (!asked && pwFields().length) { asked = true; post({ t: "pw-query", url: location.href }); }
    }
    document.addEventListener("focusin", (e) => {
      const el = e.target;
      if (!(el instanceof HTMLInputElement)) return;
      check();
      if (offers.length && managed(el)) show(el); else hide();
    }, true);
    document.addEventListener("mousedown", (e) => { if (box && !e.composedPath().includes(box)) hide(); }, true);
    // Tangkap login yang dikirim: submit formulir, klik tombol, atau Enter di kolom sandi.
    function capture() {
      const pw = pwFields().find((p) => p.value);
      if (!pw) return;
      const u = userFieldFor(pw);
      const key = (u ? u.value : "") + "\u0000" + pw.value;
      if (key === lastSent) return;
      lastSent = key;
      post({ t: "pw-capture", user: u ? u.value : "", pass: pw.value, url: location.href });
    }
    document.addEventListener("submit", capture, true);
    document.addEventListener("click", (e) => {
      const b = e.target instanceof Element && e.target.closest('button,input[type="submit"],[role="button"]');
      if (b) setTimeout(capture, 0);
    }, true);
    document.addEventListener("keydown", (e) => { if (e.key === "Enter" && e.target instanceof HTMLInputElement) capture(); }, true);
    addEventListener("pagehide", capture);
    onReady(() => {
      check();
      let n = 0;
      const mo = new MutationObserver(() => { if (!asked && ++n < 400) check(); else if (asked) mo.disconnect(); });
      mo.observe(document.documentElement, { childList: true, subtree: true });
    });
  })();

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
  // Cadangan: situs yang memakai salinan pushState sendiri (YouTube) — periksa alamat tiap
  // detik selama halaman terlihat, dan saat YouTube selesai berpindah halaman.
  let lastHref = location.href;
  const checkHref = () => { if (location.href !== lastHref) { lastHref = location.href; notifyUrl(); } };
  setInterval(() => { if (!document.hidden) checkHref(); }, 1000);
  window.addEventListener("yt-navigate-finish", checkHref);

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
  function later() { if (!timer) timer = setTimeout(flush, document.hidden ? 2000 : 250); }
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

// Tombol "Unduh video" di atas video (seperti panel unduh Mister Download Manager): muncul di
// pojok kanan atas video saat kursor di atasnya; menu ▾: sembunyikan di video ini, jangan di
// situs ini, pengaturan tombol. Ringan: hanya bereaksi pada gerak mouse (maks. ~8x/detik).
// Dibuat tanpa innerHTML (YouTube & situs Google memakai Trusted Types yang menolaknya: dulu
// tombolnya diam-diam tidak pernah jadi di YouTube) dan juga berjalan di dalam iframe (pemutar
// video situs berita sering berupa iframe); klik dari iframe diteruskan lewat bingkai utama.
(function () {
  const cfg = window.__ovnerCfg || {};
  if (!cfg.videoBtn || !/^https?:$/.test(location.protocol) || window.__ovnerVid) return;
  try { Object.defineProperty(window, "__ovnerVid", { value: true }); } catch (e) { return; }
  const top = window.top === window;
  const host = (top ? location.hostname : (function () { try { return window.top.location.hostname; } catch (e) { return location.hostname; } })()).replace(/^www\./, "").toLowerCase();
  if ((cfg.videoNever || []).some((h) => host === h || host.endsWith("." + h))) return;
  const cw = window.chrome && window.chrome.webview;
  const send = top && cw && cw.postMessage ? cw.postMessage.bind(cw) : null;
  const ipc = window.ipc;
  const post = (o) => {
    try {
      if (!top) { window.top.postMessage({ __ovnerVidMsg: o }, "*"); return; }
      const s = JSON.stringify(o); if (send) send(s); else ipc.postMessage(s);
    } catch (e) {}
  };
  if (top) {
    // Pesan dari tombol di dalam iframe: diteruskan ke Ovner dengan alamat halaman utama.
    addEventListener("message", (e) => {
      const o = e.data && e.data.__ovnerVidMsg;
      if (!o || typeof o !== "object" || typeof o.t !== "string" || !/^video-(dl|never|settings)$/.test(o.t)) return;
      post({ t: o.t, page: location.href, frame: String(o.page || "").slice(0, 2000), src: String(o.src || "").slice(0, 4000) });
    });
  }
  const id = cfg.lang !== "en";
  const W = id
    ? { dl: "Unduh video", hide: "Sembunyikan di video ini", never: "Jangan di " + host, set: "Pengaturan tombol…" }
    : { dl: "Download video", hide: "Hide on this video", never: "Never on " + host, set: "Button settings…" };
  const vids = document.getElementsByTagName("video");
  const hidden = new WeakSet();
  let hostEl = null, box = null, menu = null, cur = null, hideT = 0, off = false, menuOpen = false, last = 0, raf = 0;
  const mk = (tag, cls, text) => { const e = document.createElement(tag); if (cls) e.className = cls; if (text) e.textContent = text; return e; };

  function build() {
    hostEl = document.createElement("ovner-video-button");
    hostEl.style.cssText = "all:initial;position:fixed;left:0;top:0;width:0;height:0;z-index:2147483647;pointer-events:none";
    const root = hostEl.attachShadow({ mode: "closed" });
    const st = document.createElement("style");
    st.textContent = `
      .b{position:fixed;display:none;align-items:stretch;pointer-events:auto;font:500 13px/1 "Segoe UI",system-ui,sans-serif;color:#1f2328;
         background:#f6f7f9;border:1px solid rgba(0,0,0,.18);border-radius:10px;box-shadow:0 4px 16px rgba(0,0,0,.35);overflow:hidden;user-select:none}
      .b button{all:unset;cursor:pointer;display:flex;align-items:center;gap:7px;padding:8px 12px}
      .b button:hover{background:#e6eaf0}
      .b .m{padding:8px 8px;border-left:1px solid rgba(0,0,0,.14)}
      svg{width:16px;height:16px;flex:none}
      .menu{position:fixed;display:none;pointer-events:auto;min-width:220px;padding:6px;font:400 13px/1.2 "Segoe UI",system-ui,sans-serif;color:#1f2328;
         background:#fbfbfc;border:1px solid rgba(0,0,0,.18);border-radius:12px;box-shadow:0 8px 28px rgba(0,0,0,.35)}
      .menu button{all:unset;display:block;box-sizing:border-box;width:100%;padding:8px 12px;border-radius:8px;cursor:pointer}
      .menu button:hover{background:#e6eaf0}
      .menu hr{border:0;border-top:1px solid rgba(0,0,0,.12);margin:4px 2px}`;
    box = mk("div", "b");
    const d = mk("button", "d");
    d.append(__ovnerSvg("0 0 24 24", [["path", { d: "M12 4v11M7 10l5 5 5-5M5 20h14" }]], { fill: "none", stroke: "#1a73e8", "stroke-width": 2.2, "stroke-linecap": "round", "stroke-linejoin": "round" }), mk("span", "", W.dl));
    const m = mk("button", "m");
    m.setAttribute("aria-label", "menu");
    m.append(__ovnerSvg("0 0 24 24", [["path", { d: "M7 10l5 5 5-5" }]], { fill: "none", stroke: "currentColor", "stroke-width": 2 }));
    box.append(d, m);
    menu = mk("div", "menu");
    for (const a of ["dl", "-", "hide", "never", "-", "set"]) {
      if (a === "-") { menu.append(document.createElement("hr")); continue; }
      const b = mk("button", "", W[a]); b.dataset.a = a; menu.append(b);
    }
    root.append(st, box, menu);
    d.onclick = (e) => { e.stopPropagation(); download(); };
    m.onclick = (e) => {
      e.stopPropagation();
      menuOpen = !menuOpen;
      menu.style.display = menuOpen ? "block" : "none";
      if (menuOpen) {
        const r = box.getBoundingClientRect();
        menu.style.top = r.bottom + 4 + "px";
        menu.style.left = Math.max(4, r.right - menu.offsetWidth) + "px";
      }
    };
    menu.onclick = (e) => {
      const a = e.target.dataset && e.target.dataset.a;
      if (!a) return;
      e.stopPropagation();
      if (a === "dl") download();
      else if (a === "hide") { if (cur) hidden.add(cur); hide(); }
      else if (a === "never") { off = true; hide(); post({ t: "video-never", page: location.href }); }
      else if (a === "set") { hide(); post({ t: "video-settings", page: location.href }); }
    };
    for (const el of [box, menu]) {
      // Klik di tombol tidak sampai ke pemutar video halaman (jeda/putar).
      for (const ev of ["mousedown", "mouseup", "pointerdown", "pointerup", "click", "dblclick"]) el.addEventListener(ev, (e) => e.stopPropagation());
      el.addEventListener("mouseenter", () => clearTimeout(hideT));
    }
    (document.body || document.documentElement).appendChild(hostEl);
  }
  function download() {
    const v = cur;
    hide();
    const src = v && /^https?:/.test(v.currentSrc || "") ? v.currentSrc : "";
    post({ t: "video-dl", page: location.href, src });
  }
  function place() {
    if (!cur || !box) return;
    const r = cur.getBoundingClientRect();
    if (!r.width || r.bottom < 0 || r.top > innerHeight) { hide(); return; }
    box.style.left = Math.max(4, Math.min(r.right, innerWidth) - box.offsetWidth - 12) + "px";
    box.style.top = Math.max(r.top, 0) + 12 + "px";
  }
  function show(v) {
    if (!box) build();
    if (cur !== v) { menuOpen = false; menu.style.display = "none"; }
    cur = v;
    box.style.display = "flex";
    place();
    clearTimeout(hideT);
  }
  function hide() {
    clearTimeout(hideT);
    if (!box) return;
    box.style.display = "none";
    menu.style.display = "none";
    menuOpen = false;
    cur = null;
  }
  // YouTube: hanya di halaman video/Shorts (bukan pratinjau video di beranda/hasil pencarian).
  const ytList = () => /(^|\.)youtube\.com$/.test(location.hostname) && !/^\/(watch|shorts|live|embed)/.test(location.pathname);
  function check(x, y) {
    if (off || document.fullscreenElement || ytList()) { if (cur) hide(); return; }
    for (const v of vids) {
      if (hidden.has(v)) continue;
      const r = v.getBoundingClientRect();
      if (r.width < 200 || r.height < 110) continue;
      if (x >= r.left && x <= r.right && y >= r.top && y <= r.bottom) { show(v); return; }
    }
    if (cur && !menuOpen) { clearTimeout(hideT); hideT = setTimeout(hide, 1200); }
  }
  document.addEventListener("mousemove", (e) => {
    const now = e.timeStamp;
    if (now - last < 120 || !vids.length) return;
    last = now;
    check(e.clientX, e.clientY);
  }, { capture: true, passive: true });
  document.addEventListener("mouseleave", () => { if (cur && !menuOpen) { clearTimeout(hideT); hideT = setTimeout(hide, 800); } });
  document.addEventListener("click", (e) => { if (menuOpen && !e.composedPath().includes(hostEl)) { menuOpen = false; menu.style.display = "none"; } }, true);
  const follow = () => { if (cur && !raf) raf = requestAnimationFrame(() => { raf = 0; place(); }); };
  addEventListener("scroll", follow, { capture: true, passive: true });
  addEventListener("resize", follow, { passive: true });
})();
