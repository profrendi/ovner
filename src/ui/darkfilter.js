// Mode gelap "Filter" (seperti mode Filter di Dark Reader): halaman dibalik warnanya,
// gambar/video dibalik lagi supaya tetap asli. Cepat dan cocok untuk situs yang rewel
// dengan mode Dinamis. `__OVNER_DARK__` diganti Ovner dengan setelan kecerahan dsb.
(function (cfg) {
  const id = "__ovner_dark_filter";
  window.__ovnerDarkFilter = function (on, c) {
    c = c || cfg;
    let st = document.getElementById(id);
    if (!on) { if (st) st.remove(); return; }
    const f = `invert(0.92) hue-rotate(180deg) brightness(${c.brightness / 100}) contrast(${c.contrast / 100}) sepia(${c.sepia / 100}) grayscale(${c.grayscale / 100})`;
    const css = `html{filter:${f}!important;background:#fff!important;min-height:100%}` +
      "img,video,picture,canvas,svg image,embed,object,[style*='background-image'],[style*='background:url']" +
      "{filter:invert(1) hue-rotate(180deg)!important}" +
      "img img,picture img,video video{filter:none!important}";
    if (!st) {
      st = document.createElement("style");
      st.id = id;
      (document.head || document.documentElement).appendChild(st);
    }
    st.textContent = css;
  };
  function apply() {
    if (!document.documentElement) return false;
    window.__ovnerDarkFilter(true, cfg);
    return true;
  }
  if (!apply()) document.addEventListener("readystatechange", apply, { once: true });
  // Tetap di urutan terakhir agar tidak dikalahkan CSS situs.
  document.addEventListener("DOMContentLoaded", () => {
    const st = document.getElementById(id);
    if (st && st.parentNode) (document.head || document.documentElement).appendChild(st);
  }, { once: true });
})(__OVNER_DARK__);
