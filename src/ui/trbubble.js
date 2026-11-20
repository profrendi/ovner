// Gelembung terjemahan teks yang dipilih (disuntikkan hanya saat dipakai).
// Muncul tepat di bawah teks yang diblok, berisi pilihan bahasa tujuan, hasil, tombol Salin
// dan Tutup. Dibangun dengan DOM (tanpa innerHTML, aman di situs Trusted Types seperti YouTube)
// di dalam shadow DOM tertutup supaya gaya situs tidak memengaruhinya.
(function () {
  if (window.__ovnerTrB) return;
  const post = (o) => {
    try {
      if (window.__ovnerBridge) window.__ovnerBridge(o);
      else window.chrome.webview.postMessage(JSON.stringify(o));
    } catch (e) {}
  };
  let host = null, root = null, box = null, anchor = null, cur = null, hooked = false;
  const el = (tag, cls, txt) => { const e = document.createElement(tag); if (cls) e.className = cls; if (txt != null) e.textContent = txt; return e; };
  const dark = () => { try { return matchMedia("(prefers-color-scheme: dark)").matches; } catch (e) { return false; } };
  const CSS = `
    :host { all: initial; }
    .b { box-sizing: border-box; font: 14px/1.5 "Segoe UI Variable Text", "Segoe UI", system-ui, sans-serif; color: var(--t); background: var(--bg);
         border: 1px solid var(--ln); border-radius: 14px; box-shadow: 0 12px 32px rgba(0,0,0,.22); padding: 10px 12px 12px; }
    .h { display: flex; align-items: center; gap: 8px; margin-bottom: 6px; font-size: 12px; color: var(--m); }
    .h .f { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
    select { font: inherit; font-size: 12px; color: var(--t); background: var(--fl); border: 1px solid var(--ln); border-radius: 8px; padding: 3px 6px; max-width: 170px; }
    button { font: inherit; font-size: 12px; color: var(--t); background: var(--fl); border: 1px solid var(--ln); border-radius: 8px; padding: 4px 10px; cursor: pointer; }
    button:hover { border-color: var(--ac); }
    .x { border: 0; background: transparent; font-size: 15px; padding: 0 4px; color: var(--m); }
    .o { max-height: 260px; overflow: auto; white-space: pre-wrap; overflow-wrap: anywhere; user-select: text; padding: 2px 0 8px; }
    .busy { color: var(--m); }
    .err { color: #e5484d; }
    .ft { display: flex; gap: 8px; justify-content: flex-end; }
  `;
  function theme() {
    const d = dark();
    const v = d ? { t: "#ececf1", bg: "#202124", ln: "#3c4043", fl: "#2b2d31", m: "#a3a7ad", ac: "#8ab4f8" } : { t: "#1f2328", bg: "#ffffff", ln: "#dfe3e8", fl: "#f4f6f8", m: "#5f6b7a", ac: "#2563eb" };
    for (const k in v) box.style.setProperty("--" + k, v[k]);
  }
  function build() {
    host = document.createElement("ovner-tr-bubble");
    host.style.cssText = "position:absolute;top:0;left:0;z-index:2147483647;display:block;";
    root = host.attachShadow({ mode: "closed" });
    const st = document.createElement("style");
    st.textContent = CSS;
    box = el("div", "b");
    root.append(st, box);
    (document.body || document.documentElement).append(host);
    // Pendengar dipasang sekali saja (gelembung bisa dibuat ulang berkali-kali).
    if (!hooked) {
      hooked = true;
      document.addEventListener("mousedown", (e) => { if (host && host.isConnected && !e.composedPath().includes(host)) close(); }, true);
      document.addEventListener("keydown", (e) => { if (host && e.key === "Escape") close(); }, true);
    }
  }
  function selRect() {
    try {
      const s = getSelection();
      if (s && s.rangeCount) {
        const r = s.getRangeAt(0).getBoundingClientRect();
        if (r.width || r.height) return { left: r.left + scrollX, top: r.top + scrollY, bottom: r.bottom + scrollY };
      }
    } catch (e) {}
    return { left: scrollX + innerWidth / 2 - 180, top: scrollY + 80, bottom: scrollY + 100 };
  }
  function place() {
    if (!host || !anchor) return;
    const w = Math.min(420, innerWidth - 16);
    box.style.width = w + "px";
    let x = Math.max(scrollX + 8, Math.min(anchor.left, scrollX + innerWidth - w - 8));
    let y = anchor.bottom + 8;
    const h = box.offsetHeight || 140;
    // Tidak muat di bawah: tampilkan di atas teks.
    if (y + h > scrollY + innerHeight - 8 && anchor.top - h - 8 > scrollY) y = anchor.top - h - 8;
    host.style.left = x + "px";
    host.style.top = y + "px";
  }
  function close() {
    if (host) host.remove();
    host = null; cur = null;
  }
  function render(o) {
    cur = o;
    if (!host || !host.isConnected) build();
    theme();
    box.textContent = "";
    const en = !!o.en;
    const h = el("div", "h");
    const from = el("span", "f", o.from ? (en ? "From " : "Dari ") + o.fromName : (en ? "Translation" : "Terjemahan"));
    const sel = el("select");
    for (const [code, name] of o.langs || []) {
      const op = el("option", null, name); op.value = code; if (code === o.to) op.selected = true; sel.append(op);
    }
    sel.title = en ? "Translate to" : "Terjemahkan ke";
    sel.onchange = () => { render(Object.assign({}, cur, { to: sel.value, busy: true, out: "", err: "" })); post({ t: "tr-sel", text: cur.text, to: sel.value }); };
    const x = el("button", "x", "✕"); x.title = en ? "Close (Esc)" : "Tutup (Esc)"; x.onclick = close;
    h.append(from, el("span", null, "→"), sel, x);
    const out = el("div", "o" + (o.busy ? " busy" : o.err ? " err" : ""), o.busy ? (en ? "Translating…" : "Menerjemahkan…") : o.err ? o.err : o.out || "");
    const ft = el("div", "ft");
    if (!o.busy && !o.err && o.out) {
      const cp = el("button", null, en ? "Copy" : "Salin");
      cp.onclick = () => {
        const done = () => { cp.textContent = en ? "Copied" : "Tersalin"; };
        try { navigator.clipboard.writeText(o.out).then(done, () => { post({ t: "tr-copy", text: o.out }); done(); }); } catch (e) { post({ t: "tr-copy", text: o.out }); done(); }
      };
      ft.append(cp);
    }
    const cl = el("button", null, en ? "Close" : "Tutup"); cl.onclick = close; ft.append(cl);
    box.append(h, out, ft);
    place();
  }
  window.__ovnerTrB = {
    // Dipanggil saat menu "Terjemahkan teks yang dipilih" dipilih: catat posisi teks dulu.
    open(o) { anchor = selRect(); render(Object.assign({ busy: true }, o)); },
    show(o) { if (cur && o.text !== cur.text) return; render(Object.assign({}, cur || {}, o, { busy: false })); },
    close,
    // Untuk uji otomatis: keadaan gelembung sekarang.
    peek() { return JSON.stringify({ open: !!(host && host.isConnected), busy: !!(cur && cur.busy), out: cur ? cur.out || "" : "", to: cur ? cur.to : "" }); },
  };
})();
