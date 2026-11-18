// Privasi Ovner yang berjalan di semua bingkai (halaman dan iframe), sebelum skrip situs:
// - menyamarkan tanda aplikasi WebView2 (chrome.webview, window.ipc) agar terlihat seperti Edge;
// - mode seragam ala Mullvad: nilai generik untuk CPU, RAM, layar, timer, dan API langka;
// - YouTube: blokir codec berat (AV1/VP9/60fps, seperti enhanced-h264ify) dan normalisasi volume;
// - kebijakan referrer halaman (melengkapi tajuk Referer yang dipangkas penyaring jaringan).
(function () {
  "use strict";
  const cfg = window.__ovnerCfg || {};
  const internal = /^(ovner:|https?:\/\/ovner\.)/.test(location.href);
  if (internal) return;
  // Frame pemeriksaan "bukan robot": jangan samarkan layar/WebView2 (lihat farble.js).
  if (/(^|\.)(challenges\.cloudflare\.com|hcaptcha\.com|recaptcha\.net)$/.test(location.hostname) ||
      (/(^|\.)google\.com$/.test(location.hostname) && location.pathname.startsWith("/recaptcha"))) return;
  // Situs video: jangan ubah rasio piksel, ukuran layar, dan timer (pemutar memilih
  // resolusi dan menjadwalkan bingkai dari nilai-nilai ini).
  const media = /(^|\.)(youtube|youtube-nocookie|googlevideo|ytimg|twitch|netflix|primevideo|disneyplus|hotstar|vidio|viu|iq|wetv|dailymotion|vimeo)\.(com|tv|id)$/.test(location.hostname);
  const def = (obj, prop, value) => {
    try { Object.defineProperty(obj, prop, { get: () => value, configurable: true }); } catch (e) {}
  };

  // ---- YouTube: pilih codec (seperti ekstensi enhanced-h264ify) ----
  // YouTube bertanya ke browser codec mana yang bisa diputar; codec yang diblokir dijawab
  // "tidak bisa" sehingga YouTube mengirim codec lain. Selain isTypeSupported dan canPlayType
  // (yang dipakai enhanced-h264ify), MediaCapabilities.decodingInfo dan WebCodecs ikut dijawab
  // supaya YouTube tidak tetap memilih AV1 lewat jalur pemeriksaan yang lebih baru.
  const yt = cfg.yt || {};
  const ytHost = /(^|\.)(youtube|youtube-nocookie)\.com$/.test(location.hostname);
  const block = (yt.block || []).map((t) => String(t).toLowerCase());
  if (ytHost && (block.length || yt.no60)) {
    const bad = (t) => {
      t = String(t == null ? "" : t).toLowerCase();
      if (block.some((b) => t.includes(b))) return true;
      if (yt.no60) { const m = /framerate=["']?(\d+)/.exec(t); if (m && +m[1] > 30) return true; }
      return false;
    };
    for (const MS of [window.MediaSource, window.ManagedMediaSource, window.WebKitMediaSource]) {
      if (MS && MS.isTypeSupported) {
        const orig = MS.isTypeSupported.bind(MS);
        try { MS.isTypeSupported = function (t) { return bad(t) ? false : orig(t); }; } catch (e) {}
      }
    }
    const cpt = HTMLMediaElement.prototype.canPlayType;
    HTMLMediaElement.prototype.canPlayType = function (t) { return bad(t) ? "" : cpt.call(this, t); };
    const MC = window.MediaCapabilities && MediaCapabilities.prototype;
    if (MC && MC.decodingInfo) {
      const di = MC.decodingInfo;
      MC.decodingInfo = function (c) {
        try {
          const v = c && c.video, a = c && c.audio;
          if ((v && (bad(v.contentType) || (yt.no60 && +v.framerate > 30))) || (a && bad(a.contentType))) {
            return Promise.resolve({ supported: false, smooth: false, powerEfficient: false, configuration: c });
          }
        } catch (e) {}
        return di.apply(this, arguments);
      };
    }
    for (const D of [window.VideoDecoder, window.AudioDecoder]) {
      if (D && D.isConfigSupported) {
        const ics = D.isConfigSupported.bind(D);
        try {
          D.isConfigSupported = function (c) {
            if (c && (bad(c.codec) || (yt.no60 && +c.framerate > 30))) return Promise.resolve({ supported: false, config: c });
            return ics(c);
          };
        } catch (e) {}
      }
    }
  }
  // Normalisasi volume YouTube dimatikan: volume video mengikuti slider pemutar
  // (enhanced-h264ify "Disable Loudness Normalization").
  if (ytHost && yt.noLN) {
    let panel = null, burst = 0, burstAt = 0;
    const want = () => { const v = panel && +panel.getAttribute("aria-valuenow"); return v >= 0 && v <= 100 ? v / 100 : null; };
    const apply = () => {
      const v = document.querySelector("video.html5-main-video, video"), w = want();
      if (!v || w == null || v.muted || Math.abs(v.volume - w) < 0.005) return;
      // Pengaman: bila pemutar terus membalas perubahan ini, berhenti sebentar (tidak saling kejar).
      const now = Date.now();
      if (now - burstAt > 5000) { burst = 0; burstAt = now; }
      if (++burst > 4) return;
      try { v.volume = w; } catch (e) {}
    };
    const watch = () => {
      const p = document.querySelector(".ytp-volume-panel");
      if (!p || p === panel) return;
      panel = p;
      new MutationObserver(apply).observe(p, { attributes: true, attributeFilter: ["aria-valuenow"] });
      apply();
    };
    // YouTube menurunkan volume lagi setiap video baru dimuat: terapkan ulang saat itu.
    document.addEventListener("loadedmetadata", () => setTimeout(apply, 250), true);
    document.addEventListener("playing", () => setTimeout(apply, 250), true);
    const t = setInterval(() => { watch(); if (panel) clearInterval(t); }, 1000);
    document.addEventListener("yt-navigate-finish", watch, true);
  }

  // ---- Referrer ----
  if (window.top === window && cfg.referrer && cfg.referrer !== "default") {
    const policy = "strict-origin";
    const add = () => {
      const root = document.head || document.documentElement;
      if (!root) return false;
      const m = document.createElement("meta");
      m.name = "referrer"; m.content = policy;
      root.prepend(m);
      return true;
    };
    if (!add()) document.addEventListener("readystatechange", add, { once: true });
  }

  // ---- Mode seragam (seperti Mullvad Browser) ----
  // Situs kompatibilitas/pengecualian: tanpa mode seragam (lihat farbleOff).
  const host = location.hostname.replace(/^www\./, "");
  const exempt = (cfg.farbleOff || []).some((h) => host === h || host.endsWith("." + h));
  if (cfg.uniform && !exempt) {
    const NP = Navigator.prototype;
    def(NP, "hardwareConcurrency", 8);
    def(NP, "deviceMemory", 8);
    try { NP.getGamepads = function () { return []; }; } catch (e) {}
    try { NP.getBattery = undefined; } catch (e) {}
    if (window.speechSynthesis) { try { speechSynthesis.getVoices = () => []; } catch (e) {} }
    for (const n of ["Accelerometer", "Gyroscope", "Magnetometer", "AbsoluteOrientationSensor", "RelativeOrientationSensor",
      "LinearAccelerationSensor", "GravitySensor", "AmbientLightSensor"]) {
      try { delete window[n]; } catch (e) {}
    }
    // Timer dibulatkan ke 1 ms (Mullvad: 1000 mikrodetik), kecuali situs video.
    if (!media) {
      const now = Performance.prototype.now;
      Performance.prototype.now = function () { return Math.floor(now.call(this)); };
    }
    // Layar dilaporkan sebesar jendela, dibulatkan ke kelipatan 200×100.
    const SP = Screen.prototype;
    const w = () => Math.max(200, Math.floor(window.innerWidth / 200) * 200);
    const h = () => Math.max(100, Math.floor(window.innerHeight / 100) * 100);
    if (!media) {
      for (const [p, f] of [["width", w], ["availWidth", w], ["height", h], ["availHeight", h]]) {
        try { Object.defineProperty(SP, p, { get: f, configurable: true }); } catch (e) {}
      }
      def(window, "devicePixelRatio", 1);
    }
    def(SP, "colorDepth", 24);
    def(SP, "pixelDepth", 24);
  }

  // ---- Samarkan WebView2 ----
  if (cfg.hideWv) {
    try { delete window.ipc; } catch (e) {}
    try {
      if (window.chrome && "webview" in window.chrome && !delete window.chrome.webview) {
        Object.defineProperty(window.chrome, "webview", { value: undefined, configurable: true, enumerable: false });
      }
    } catch (e) {}
  }
})();
