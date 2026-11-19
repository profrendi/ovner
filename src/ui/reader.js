// Baca keras Ovner, bagian halaman (disuntikkan hanya saat Baca keras dimulai): mengambil
// paragraf yang bisa dibaca dan menyorot paragraf/kata yang sedang dibaca. Suaranya diputar
// oleh toolbar Ovner (suara Windows atau suara online), jadi halaman tidak perlu izin audio.
// Sorotan memakai CSS Custom Highlight API: isi halaman tidak diubah sama sekali.
(function () {
  if (window.__ovnerRead) return;
  const post = (o) => {
    try {
      if (window.__ovnerBridge) window.__ovnerBridge(o);
      else window.chrome.webview.postMessage(JSON.stringify(o));
    } catch (e) {}
  };
  const BLOCK = /^(P|H[1-6]|LI|BLOCKQUOTE|FIGCAPTION|DD|DT|TD|TH|CAPTION|SUMMARY|LABEL|DIV|SECTION|ARTICLE|MAIN|PRE)$/;
  const NOISE = "nav,footer,aside,form,button,select,textarea,input,script,style,noscript,svg,canvas,iframe,[role=navigation],[role=banner],[role=complementary],[aria-hidden=true],ovner-video-button";
  const R = { paras: [], n: 0, sheet: null };
  function root() {
    const cands = [...document.querySelectorAll("article, main, [role=main], #content, .content, .post, .entry-content, .article-body")];
    let best = null, len = 0;
    for (const c of cands) { const l = (c.innerText || "").length; if (l > len) { best = c; len = l; } }
    return best && len > 400 ? best : document.body;
  }
  function blockOf(n) {
    for (let e = n.parentElement; e; e = e.parentElement) if (BLOCK.test(e.tagName)) return e;
    return document.body;
  }
  function shown(el) {
    if (!el || !el.getClientRects().length) return false;
    const st = getComputedStyle(el);
    return st.visibility !== "hidden" && st.display !== "none";
  }
  function build(rootEl, range) {
    const paras = [];
    let cur = null;
    const w = document.createTreeWalker(rootEl, NodeFilter.SHOW_TEXT, {
      acceptNode(n) {
        if (!n.nodeValue || !n.nodeValue.trim()) return NodeFilter.FILTER_REJECT;
        if (range && !range.intersectsNode(n)) return NodeFilter.FILTER_REJECT;
        const p = n.parentElement;
        if (!p || p.closest(NOISE)) return NodeFilter.FILTER_REJECT;
        return NodeFilter.FILTER_ACCEPT;
      },
    });
    const visCache = new Map();
    for (let n = w.nextNode(); n; n = w.nextNode()) {
      const b = blockOf(n);
      if (!visCache.has(b)) visCache.set(b, shown(b));
      if (!visCache.get(b)) continue;
      const raw = n.nodeValue;
      let off = 0, end = raw.length;
      if (range && n === range.startContainer) off = range.startOffset;
      if (range && n === range.endContainer) end = range.endOffset;
      if (end <= off) continue;
      // Spasi diganti satu-satu (panjang sama) supaya posisi kata = posisi di simpul aslinya.
      const piece = raw.slice(off, end).replace(/\s/g, " ");
      if (!cur || cur.block !== b) {
        cur = { block: b, text: "", parts: [] };
        paras.push(cur);
      }
      if (cur.text && !cur.text.endsWith(" ") && !piece.startsWith(" ")) cur.text += " ";
      cur.parts.push({ node: n, at: cur.text.length, len: piece.length, off });
      cur.text += piece;
    }
    return paras.filter((p) => /\p{L}|\d/u.test(p.text) && p.text.trim().length > 1);
  }
  function sheet() {
    if (R.sheet) return;
    try {
      R.sheet = new CSSStyleSheet();
      R.sheet.replaceSync("::highlight(ovner-read-p){background-color:rgba(255,213,74,.28)}::highlight(ovner-read-w){background-color:#ffd54a;color:#111}");
      document.adoptedStyleSheets = [...document.adoptedStyleSheets, R.sheet];
    } catch (e) {}
  }
  // Posisi teks paragraf → (simpul, offset) di halaman.
  function point(p, pos) {
    for (const part of p.parts) {
      if (pos <= part.at + part.len) return [part.node, part.off + Math.max(0, pos - part.at)];
    }
    const last = p.parts[p.parts.length - 1];
    return [last.node, last.off + last.len];
  }
  function rangeOf(p, a, b) {
    try {
      const r = document.createRange();
      const [n1, o1] = point(p, a), [n2, o2] = point(p, b);
      r.setStart(n1, o1); r.setEnd(n2, o2);
      return r;
    } catch (e) { return null; }
  }
  R.start = function (n, selection) {
    R.stop();
    R.n = n;
    let range = null;
    const sel = getSelection();
    if (selection && sel && sel.rangeCount && !sel.isCollapsed) range = sel.getRangeAt(0);
    R.paras = build(range ? range.commonAncestorContainer.nodeType === 1 ? range.commonAncestorContainer : range.commonAncestorContainer.parentElement : root(), range);
    let start = 0;
    if (!range) {
      // Mulai dari paragraf pertama yang terlihat di layar (seperti Read aloud Edge).
      for (let i = 0; i < R.paras.length; i++) {
        const r = R.paras[i].block.getBoundingClientRect();
        if (r.bottom > 60) { start = i; break; }
      }
    }
    sheet();
    post({ t: "read-text", n, start, paras: R.paras.map((p) => p.text.slice(0, 6000)), lang: (document.documentElement.lang || "").slice(0, 12), title: document.title.slice(0, 200) });
  };
  R.mark = function (n, i, a, len) {
    if (n !== R.n || !window.CSS || !CSS.highlights) return;
    const p = R.paras[i];
    if (!p) return;
    const pr = rangeOf(p, 0, p.text.length);
    if (pr) CSS.highlights.set("ovner-read-p", new Highlight(pr)); else CSS.highlights.delete("ovner-read-p");
    if (len > 0) {
      const wr = rangeOf(p, a, a + len);
      if (wr) CSS.highlights.set("ovner-read-w", new Highlight(wr));
    } else CSS.highlights.delete("ovner-read-w");
    const r = p.block.getBoundingClientRect();
    if (r.top < 70 || r.bottom > innerHeight - 40) p.block.scrollIntoView({ block: "center", behavior: "smooth" });
  };
  R.stop = function () {
    try { CSS.highlights.delete("ovner-read-p"); CSS.highlights.delete("ovner-read-w"); } catch (e) {}
  };
  Object.defineProperty(window, "__ovnerRead", { value: R, configurable: false });
})();
