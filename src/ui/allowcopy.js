// Izinkan salin (seperti Allow Copy+): situs tidak bisa lagi melarang pilih teks, salin,
// potong, tempel, seret, dan klik kanan. Dipasang per situs dari tameng Ovner.
(function () {
  if (window.__ovnerAllowCopy) return;
  Object.defineProperty(window, "__ovnerAllowCopy", { value: true });
  const TYPES = new Set(["copy", "cut", "paste", "contextmenu", "selectstart", "dragstart", "beforecopy", "beforecut", "beforepaste"]);
  // 1. Skrip situs tidak menerima event ini sama sekali (kita menangkap paling awal di window).
  const stop = (e) => { e.stopImmediatePropagation(); };
  for (const t of TYPES) window.addEventListener(t, stop, true);
  // Klik kanan dan Ctrl+C/X/V/A sering dibatalkan lewat mousedown/keydown.
  window.addEventListener("mousedown", (e) => { if (e.button === 2) e.stopImmediatePropagation(); }, true);
  window.addEventListener("mouseup", (e) => { if (e.button === 2) e.stopImmediatePropagation(); }, true);
  window.addEventListener("keydown", (e) => {
    const k = (e.key || "").toLowerCase();
    if ((e.ctrlKey || e.metaKey) && !e.altKey && ["c", "x", "v", "a", "insert"].includes(k)) e.stopImmediatePropagation();
    if (e.shiftKey && k === "insert") e.stopImmediatePropagation();
  }, true);
  // 2. preventDefault untuk event tersebut diabaikan (mode mutlak).
  const pd = Event.prototype.preventDefault;
  Event.prototype.preventDefault = function () {
    if (TYPES.has(this.type)) return;
    return pd.call(this);
  };
  try {
    const rv = Object.getOwnPropertyDescriptor(Event.prototype, "returnValue");
    if (rv && rv.set) Object.defineProperty(Event.prototype, "returnValue", { get: rv.get, set(v) { if (TYPES.has(this.type) && v === false) return; rv.set.call(this, v); }, configurable: true });
  } catch (e) {}
  // 3. Teks bisa dipilih lagi walau situs memasang user-select: none.
  const css = "*,*::before,*::after{-webkit-user-select:text!important;user-select:text!important;-webkit-touch-callout:default!important}" +
    "html,body{-webkit-user-drag:auto!important}";
  function addStyle() {
    const root = document.documentElement;
    if (!root) return false;
    if (document.getElementById("__ovner_allowcopy")) return true;
    const st = document.createElement("style");
    st.id = "__ovner_allowcopy";
    st.textContent = css;
    root.appendChild(st);
    return true;
  }
  if (!addStyle()) document.addEventListener("readystatechange", addStyle, { once: true });
  document.addEventListener("DOMContentLoaded", () => {
    addStyle();
    for (const el of [document, document.body, document.documentElement]) {
      if (!el) continue;
      for (const t of TYPES) { try { el["on" + t] = null; } catch (e) {} }
    }
  }, { once: true });
})();
