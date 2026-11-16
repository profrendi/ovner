// Privasi Ovner yang berjalan di semua bingkai (halaman dan iframe), sebelum skrip situs:
// - menyamarkan tanda aplikasi WebView2 (chrome.webview, window.ipc) agar terlihat seperti Edge;
// - mode seragam ala Mullvad: nilai generik untuk CPU, RAM, layar, timer, dan API langka;
// - YouTube memakai H.264 (lebih ringan didekode CPU, berguna saat GPU dimatikan);
// - kebijakan referrer halaman (melengkapi tajuk Referer yang dipangkas penyaring jaringan).
(function () {
  "use strict";
  const cfg = window.__ovnerCfg || {};
  const internal = /^(ovner:|https?:\/\/ovner\.)/.test(location.href);
  if (internal) return;
  const def = (obj, prop, value) => {
    try { Object.defineProperty(obj, prop, { get: () => value, configurable: true }); } catch (e) {}
  };

  // ---- YouTube: H.264 saja (seperti ekstensi h264ify) ----
  if (cfg.h264 && /(^|\.)(youtube|youtube-nocookie)\.com$/.test(location.hostname)) {
    const heavy = /vp0?9|vp8|av01/i;
    for (const MS of [window.MediaSource, window.ManagedMediaSource]) {
      if (MS && MS.isTypeSupported) {
        const orig = MS.isTypeSupported.bind(MS);
        try { MS.isTypeSupported = (t) => (heavy.test(String(t)) ? false : orig(t)); } catch (e) {}
      }
    }
    const cpt = HTMLMediaElement.prototype.canPlayType;
    HTMLMediaElement.prototype.canPlayType = function (t) { return heavy.test(String(t)) ? "" : cpt.call(this, t); };
  }

  // ---- Referrer ----
  if (window.top === window && cfg.referrer && cfg.referrer !== "default") {
    const policy = cfg.referrer === "none" ? "same-origin" : "strict-origin";
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
  if (cfg.uniform) {
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
    // Timer dibulatkan ke 1 ms (Mullvad: 1000 mikrodetik).
    const now = Performance.prototype.now;
    Performance.prototype.now = function () { return Math.floor(now.call(this)); };
    // Layar dilaporkan sebesar jendela, dibulatkan ke kelipatan 200×100.
    const SP = Screen.prototype;
    const w = () => Math.max(200, Math.floor(window.innerWidth / 200) * 200);
    const h = () => Math.max(100, Math.floor(window.innerHeight / 100) * 100);
    for (const [p, f] of [["width", w], ["availWidth", w], ["height", h], ["availHeight", h]]) {
      try { Object.defineProperty(SP, p, { get: f, configurable: true }); } catch (e) {}
    }
    def(SP, "colorDepth", 24);
    def(SP, "pixelDepth", 24);
    def(window, "devicePixelRatio", 1);
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
