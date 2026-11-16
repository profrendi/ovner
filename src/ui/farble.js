// Farbling Ovner: mengacak sedikit hasil API yang dipakai untuk "sidik jari" browser
// (canvas, audio, WebGL, measureText, jumlah core CPU), dengan benih per situs per sesi.
// Situs yang sama melihat nilai yang konsisten selama sesi; situs berbeda melihat nilai
// berbeda, sehingga pelacak tidak bisa mencocokkan Anda antar situs. Perubahan pikselnya
// terlalu kecil untuk terlihat mata.
(function () {
  "use strict";
  const cfg = window.__ovnerCfg;
  if (!cfg || cfg.farbling === "off" || window.__ovnerFarbled) return;
  try { Object.defineProperty(window, "__ovnerFarbled", { value: true }); } catch (e) { return; }

  // Situs puncak (tab), juga dari dalam iframe lintas situs.
  let top = location.hostname;
  try {
    if (window.top !== window) top = window.top.location.hostname;
  } catch (e) {
    const anc = location.ancestorOrigins;
    if (anc && anc.length) { try { top = new URL(anc[anc.length - 1]).hostname; } catch (e2) {} }
  }
  top = (top || "").replace(/^www\./, "").toLowerCase();
  const off = (cfg.farbleOff || []).some((h) => top === h || top.endsWith("." + h));
  if (off || !top) return;
  // Situs video memakai mode standar: perubahan ukuran elemen dan merek GPU di mode ketat
  // bisa membuat pemutar video hitam/tidak jalan.
  const media = /(^|\.)(youtube|youtube-nocookie|googlevideo|twitch|netflix|primevideo|disneyplus|hotstar|vidio|vimeo|dailymotion)\.(com|tv|id)$/.test(top);
  const strict = cfg.farbling === "strict" && !media;

  // Domain dasar (perkiraan eTLD+1) supaya sub-domain satu situs berbagi benih.
  const parts = top.split(".");
  const base = parts.length > 2 && parts[parts.length - 2].length <= 3 && parts[parts.length - 1].length === 2
    ? parts.slice(-3).join(".") : parts.slice(-2).join(".");

  // Benih: hash FNV-1a dari kunci sesi + situs, lalu PRNG mulberry32.
  let h = 2166136261 >>> 0;
  for (const ch of cfg.key + "|" + base) { h ^= ch.charCodeAt(0); h = Math.imul(h, 16777619) >>> 0; }
  const seed = h;
  function rng(salt) {
    let a = (seed ^ Math.imul(salt >>> 0, 2654435761)) >>> 0;
    return function () {
      a = (a + 0x6d2b79f5) >>> 0;
      let t = a;
      t = Math.imul(t ^ (t >>> 15), t | 1);
      t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
      return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
    };
  }

  // Fungsi pengganti tetap terlihat "native" bila diperiksa dengan toString().
  const nativeStr = new WeakMap();
  const fnToString = Function.prototype.toString;
  function wrap(obj, name, make) {
    const orig = obj && obj[name];
    if (typeof orig !== "function") return;
    const repl = make(orig);
    nativeStr.set(repl, fnToString.call(orig));
    try { Object.defineProperty(obj, name, { value: repl, writable: true, configurable: true, enumerable: false }); } catch (e) {}
  }
  function wrapGetter(proto, name, make) {
    const d = proto && Object.getOwnPropertyDescriptor(proto, name);
    if (!d || typeof d.get !== "function") return;
    const g = make(d.get);
    nativeStr.set(g, fnToString.call(d.get));
    try { Object.defineProperty(proto, name, { get: g, set: d.set, configurable: true, enumerable: d.enumerable }); } catch (e) {}
  }
  wrap(Function.prototype, "toString", (orig) => function () {
    return nativeStr.has(this) ? nativeStr.get(this) : orig.apply(this, arguments);
  });

  // ---- Canvas: ubah beberapa piksel (standar ≤ 10 piksel, ketat lebih banyak) ----
  function farblePixels(data, w, hgt) {
    if (!data || data.length < 4) return;
    const n = data.length >> 2;
    const r = rng(n ^ (w << 16) ^ hgt);
    const count = Math.min(n, strict ? 64 : 10);
    for (let i = 0; i < count; i++) {
      const p = Math.floor(r() * n) << 2;
      const c = Math.floor(r() * 3);
      data[p + c] = data[p + c] ^ 1;
    }
  }
  const C2D = window.CanvasRenderingContext2D && CanvasRenderingContext2D.prototype;
  wrap(C2D, "getImageData", (orig) => function () {
    const img = orig.apply(this, arguments);
    try { farblePixels(img.data, img.width, img.height); } catch (e) {}
    return img;
  });
  // toDataURL/toBlob: salin ke kanvas sementara, acak, lalu ekspor (gambar asli tidak berubah).
  function farbledCopy(canvas) {
    const w = canvas.width, hgt = canvas.height;
    if (!w || !hgt || w * hgt > 4096 * 4096) return null;
    const copy = document.createElement("canvas");
    copy.width = w; copy.height = hgt;
    const ctx = copy.getContext("2d");
    if (!ctx) return null;
    ctx.drawImage(canvas, 0, 0);
    const img = ctx.getImageData(0, 0, w, hgt); // sudah diacak oleh getImageData di atas
    ctx.putImageData(img, 0, 0);
    return copy;
  }
  const HC = window.HTMLCanvasElement && HTMLCanvasElement.prototype;
  wrap(HC, "toDataURL", (orig) => function () {
    try { const c = farbledCopy(this); if (c) return orig.apply(c, arguments); } catch (e) {}
    return orig.apply(this, arguments);
  });
  wrap(HC, "toBlob", (orig) => function () {
    try { const c = farbledCopy(this); if (c) return orig.apply(c, arguments); } catch (e) {}
    return orig.apply(this, arguments);
  });

  // ---- measureText: skala lebar teks dengan faktor sangat kecil ----
  const TM = window.TextMetrics && TextMetrics.prototype;
  const tmFactor = 1 + (rng(7)() - 0.5) * (strict ? 0.00002 : 0.000006);
  wrapGetter(TM, "width", (get) => function () { return get.call(this) * tmFactor; });

  // ---- Audio: derau sangat kecil pada data yang dibaca ----
  const seen = new WeakSet();
  const AB = window.AudioBuffer && AudioBuffer.prototype;
  wrap(AB, "getChannelData", (orig) => function () {
    const d = orig.apply(this, arguments);
    try {
      if (!seen.has(d)) {
        seen.add(d);
        const r = rng(d.length);
        const step = Math.max(1, Math.floor(d.length / (strict ? 4096 : 512)));
        for (let i = Math.floor(r() * step); i < d.length; i += step) d[i] += (r() - 0.5) * 1e-7;
      }
    } catch (e) {}
    return d;
  });
  const AN = window.AnalyserNode && AnalyserNode.prototype;
  wrap(AN, "getFloatFrequencyData", (orig) => function (arr) {
    const res = orig.apply(this, arguments);
    try { const r = rng(arr.length + 11); for (let i = 0; i < arr.length; i += 8) arr[i] += (r() - 0.5) * 1e-4; } catch (e) {}
    return res;
  });

  // ---- WebGL: acak beberapa byte readPixels; ketat: sembunyikan nama GPU ----
  for (const G of [window.WebGLRenderingContext, window.WebGL2RenderingContext]) {
    if (!G) continue;
    wrap(G.prototype, "readPixels", (orig) => function () {
      const res = orig.apply(this, arguments);
      try {
        const px = arguments[6];
        if (px && px.length) farblePixels(px, arguments[2] | 0, arguments[3] | 0);
      } catch (e) {}
      return res;
    });
    if (strict) {
      wrap(G.prototype, "getParameter", (orig) => function (p) {
        if (p === 0x9245) return "Google Inc.";            // UNMASKED_VENDOR_WEBGL
        if (p === 0x9246) return "ANGLE (Generic GPU)";    // UNMASKED_RENDERER_WEBGL
        return orig.apply(this, arguments);
      });
    }
  }

  // ---- CPU dan RAM yang dilaporkan (mode seragam sudah memakai nilai tetap) ----
  const NP = window.Navigator && Navigator.prototype;
  if (!cfg.uniform) wrapGetter(NP, "hardwareConcurrency", (get) => {
    const real = get.call(navigator) || 4;
    const v = strict ? 2 : Math.max(2, Math.min(real, 2 + Math.floor(rng(3)() * (real - 1))));
    return function () { return v; };
  });
  if (strict && !cfg.uniform) {
    wrapGetter(NP, "deviceMemory", () => function () { return 4; });
  }
  if (strict) {
    // getClientRects/getBoundingClientRect: skala sangat kecil (seperti ungoogled-chromium).
    const f = 1 + (rng(13)() - 0.5) * 0.000006;
    const scaleRect = (r) => { try { return new DOMRect(r.x * f, r.y * f, r.width * f, r.height * f); } catch (e) { return r; } };
    wrap(Element.prototype, "getBoundingClientRect", (orig) => function () { return scaleRect(orig.apply(this, arguments)); });
  }
})();
