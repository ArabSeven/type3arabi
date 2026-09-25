// Type3arabi Settings UI. All reads/writes go through the Rust commands (src/main.rs).
const invoke = (cmd, args) => window.__TAURI__.core.invoke(cmd, args);
const $ = (s) => document.querySelector(s);
const $$ = (s) => [...document.querySelectorAll(s)];

let current = null;

// ---- navigation
$$("nav button[data-page]").forEach((b) =>
  b.addEventListener("click", () => {
    $$("nav button").forEach((x) => x.classList.toggle("active", x === b));
    $$(".page").forEach((p) => p.classList.toggle("active", p.id === b.dataset.page));
  })
);

// ---- binding between the Settings object and the form
function fill(s) {
  current = { ...s };
  $$("[data-key]").forEach((el) => {
    const v = s[el.dataset.key];
    if (el.type === "checkbox") el.checked = !!v;
    else el.value = v;
    el.classList.remove("invalid");
  });
  $$('[data-kind="hotkey"]').forEach((el) => (el.disabled = !s.global_hotkey_enabled));
}

function collect() {
  const s = { ...current };
  $$("[data-key]").forEach((el) => {
    const k = el.dataset.key;
    if (el.type === "checkbox") s[k] = el.checked;
    else if (el.type === "number") s[k] = parseInt(el.value, 10);
    else s[k] = el.value;
  });
  return s;
}

/** Footer message: an Arabic line and an English line (each its own bidi run), optionally a path shown
 *  once. Success and info fade after 5 s; errors stay until the next action (they say what to do). */
let statusTimer = 0;
function status(ar, en, kind, path) {
  const el = $("#status");
  clearTimeout(statusTimer);
  el.textContent = "";
  el.className = kind || "";
  if (ar) el.append(Object.assign(document.createElement("span"), { textContent: ar }));
  if (en) el.append(Object.assign(document.createElement("span"), { textContent: en, lang: "en" }));
  if (path) el.append(Object.assign(document.createElement("span"), { textContent: path, className: "path" }));
  if ((ar || en) && kind !== "err") {
    statusTimer = setTimeout(() => {
      el.classList.add("is-fading");
      statusTimer = setTimeout(() => { el.textContent = ""; el.className = ""; }, 300);
    }, 5000);
  }
}

// ---- shortcut capture
// Click a field → it waits for a shortcut. Esc (or clicking elsewhere) cancels and keeps the old
// value. An invalid or unsupported shortcut is explained and never stored: the field keeps waiting,
// so the next attempt works and Save is never blocked by a rejected attempt.
const KEY_NAMES = { " ": "Space", Enter: "Enter", Tab: "Tab", Backspace: "Backspace" };
const MODIFIERS = ["Control", "Alt", "Shift", "Meta", "AltGraph", "OS"];
const WAITING = "…اضغط الاختصار (Esc للإلغاء) / press keys (Esc cancels)";

function chordFromEvent(e, kind) {
  if (MODIFIERS.includes(e.key)) return null;
  let key = KEY_NAMES[e.key];
  if (!key && /^[a-z0-9]$/i.test(e.key)) key = e.key.toUpperCase();
  if (!key && kind === "hotkey" && /^F([1-9]|1[0-9]|2[0-4])$/.test(e.key)) key = e.key;
  if (!key) return undefined;
  const mods = [];
  if (e.ctrlKey) mods.push("Ctrl");
  if (e.altKey) mods.push("Alt");
  if (e.shiftKey) mods.push("Shift");
  if (e.metaKey && kind === "hotkey") mods.push("Win");
  return [...mods, key].join("+");
}

function stopCapture(el, value) {
  el.classList.remove("capturing");
  el.value = value;
  delete el.dataset.capturing;
  el.blur();
}

$$('input[data-kind]').forEach((el) => {
  el.addEventListener("focus", () => {
    if (el.dataset.capturing) return;
    el.dataset.capturing = "1";
    el.dataset.before = el.value;
    el.classList.add("capturing");
    el.classList.remove("invalid");
    el.value = WAITING;
  });
  el.addEventListener("blur", () => {
    if (el.dataset.capturing) {
      el.classList.remove("capturing");
      el.value = el.dataset.before;
      delete el.dataset.capturing;
    }
  });
  el.addEventListener("keydown", async (e) => {
    e.preventDefault();
    e.stopPropagation();
    if (!el.dataset.capturing || el.dataset.checking) return;
    if (e.key === "Escape" && !e.ctrlKey && !e.altKey && !e.shiftKey && !e.metaKey) {
      stopCapture(el, el.dataset.before);
      status("");
      return;
    }
    const chord = chordFromEvent(e, el.dataset.kind);
    if (chord === null) return; // a modifier on its own: keep waiting for the key
    if (chord === undefined) {
      status("هذا المفتاح غير مدعوم — جرّب غيره أو اضغط Esc", "That key is not supported — try another, or press Esc", "err");
      return;
    }
    el.dataset.checking = "1";
    let ok = false;
    try {
      ok = el.dataset.kind === "chord" ? await invoke("check_chord", { chord }) : /(Ctrl|Alt|Win)\+/.test(chord);
    } finally {
      delete el.dataset.checking;
    }
    if (!el.dataset.capturing) return; // cancelled while checking
    const same = (v) => (v || "").toLowerCase() === chord.toLowerCase();
    const clash =
      el.dataset.kind === "chord" &&
      ($$('input[data-kind="chord"]').some((o) => o !== el && same(o.value)) ||
        same($('[data-key="mode_toggle"]')?.value));
    if (!ok || clash) {
      const why = clash
        ? "it is already used by another shortcut"
        : el.dataset.kind === "hotkey"
          ? "the global hotkey needs Ctrl, Alt or Win"
          : "it would get in the way of normal typing — letters need Ctrl or Alt";
      status(`«${chord}» غير صالح — جرّب اختصاراً آخر أو Esc للإلغاء`, `"${chord}" can't be used (${why}) — try another, or press Esc`, "err");
      return; // keep waiting; nothing is stored
    }
    stopCapture(el, chord);
    status("");
  });
});

$$("button.none").forEach((b) =>
  b.addEventListener("click", () => {
    const input = b.parentElement.querySelector("input");
    input.value = "none";
    input.classList.remove("invalid");
  })
);

$('[data-key="global_hotkey_enabled"]').addEventListener("change", (e) => {
  $('[data-kind="hotkey"]').disabled = !e.target.checked;
});

// ---- actions
$("#save").addEventListener("click", async () => {
  try {
    await invoke("save_settings", { settings: collect() });
    current = collect();
    status("تم الحفظ — يطبَّق عند الكلمة التالية", "Saved — applies from the next word", "ok");
  } catch (errors) {
    status("", (Array.isArray(errors) ? errors : [String(errors)]).join("\n"), "err");
  }
});

$("#reset").addEventListener("click", async () => {
  fill(await invoke("defaults"));
  status("القيم الافتراضية — اضغط حفظ لتطبيقها", "Defaults loaded — press Save to apply");
});

$("#wipe").addEventListener("click", async () => {
  if (!confirm("نسيان كل الكلمات والاختيارات المتعلَّمة؟\u200F\nForget every learned word and choice?")) return;
  try {
    await invoke("wipe_learning");
    status("تم نسيان كل ما تعلّمه «اكتب عربي»", "Everything learned was forgotten", "ok");
  } catch (e) {
    status("", String(e), "err");
  }
});

// ---- move learning to another PC (export / import a .t3learn file)
$("#export").addEventListener("click", async () => {
  try {
    const r = await invoke("export_learning", { includeSettings: $("#export-settings").checked });
    status(`تم التصدير: ${r.records} اختياراً`, `Exported ${r.records} choices`, "ok", r.path);
    renderHistory();
  } catch (e) {
    status("", String(e), "err");
  }
});

let importBytes = null;
let importName = "";
$("#import").addEventListener("click", () => $("#import-file").click());
$("#import-file").addEventListener("change", async (e) => {
  const file = e.target.files[0];
  e.target.value = "";
  if (!file) return;
  try {
    importBytes = Array.from(new Uint8Array(await file.arrayBuffer()));
    importName = file.name;
    const p = await invoke("inspect_learning", { bytes: importBytes });
    const sum = $("#import-summary");
    sum.textContent = "";
    sum.append(Object.assign(document.createElement("span"), { textContent: `«${file.name}»: ${p.records} اختياراً متعلَّماً${p.has_settings ? " + إعدادات" : ""}` }),
      Object.assign(document.createElement("span"), { textContent: `${p.records} learned choices${p.has_settings ? " + settings" : ""}`, lang: "en" }));
    $("#import-settings-row").hidden = !p.has_settings;
    $("#import-settings").checked = false;
    $("#import-panel").hidden = false;
    status("");
  } catch (err) {
    importBytes = null;
    status("لا يمكن قراءة الملف", String(err), "err");
  }
});
$("#import-cancel").addEventListener("click", () => { importBytes = null; $("#import-panel").hidden = true; });
$("#import-go").addEventListener("click", async () => {
  if (!importBytes) return;
  const replace = document.querySelector('input[name="import-mode"]:checked').value === "replace";
  if (replace && !confirm("استبدال كل ما تعلّمه هذا الجهاز بمحتوى الملف؟\u200F\nReplace everything this PC learned with the file?")) return;
  try {
    const r = await invoke("import_learning", { bytes: importBytes, replace, restoreSettings: $("#import-settings").checked, fileName: importName });
    importBytes = null;
    $("#import-panel").hidden = true;
    if (r.settings_restored) fill(await invoke("get_settings"));
    status(`تم استيراد ${r.records} اختياراً${r.settings_restored ? " والإعدادات" : ""} — يطبَّق في كل البرامج من الكلمة التالية`,
      `Imported ${r.records} choices${r.settings_restored ? " and settings" : ""} — every app uses them from the next word`, "ok");
    renderHistory();
  } catch (err) {
    status("", String(err), "err");
  }
});

// ---- transfer history (newest first; local time)
const pad = (n) => String(n).padStart(2, "0");
const when = (t) => {
  const d = new Date(t * 1000);
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${pad(d.getHours())}:${pad(d.getMinutes())}`;
};
async function renderHistory() {
  const box = $("#history");
  let items = [];
  try { items = await invoke("transfer_history"); } catch { /* history is a convenience */ }
  box.textContent = "";
  for (const h of items) {
    const exp = h.kind === "export";
    const row = Object.assign(document.createElement("div"), { className: `hrow ${h.kind}` });
    row.setAttribute("role", "listitem");
    const ic = Object.assign(document.createElement("span"), { className: "ic", textContent: exp ? "↑" : "↓" });
    ic.setAttribute("aria-hidden", "true");
    const extra = exp ? (h.settings ? " + الإعدادات" : "") : `${h.mode === "replace" ? " · استبدال" : " · دمج"}${h.settings ? " + الإعدادات" : ""}`;
    const extraEn = exp ? (h.settings ? " + settings" : "") : `${h.mode === "replace" ? " · replaced" : " · merged"}${h.settings ? " + settings" : ""}`;
    const what = Object.assign(document.createElement("span"), { className: "what" });
    what.append(`${exp ? "تصدير" : "استيراد"} ${h.records} اختياراً${extra}`,
      Object.assign(document.createElement("small"), { textContent: `${exp ? "Export" : "Import"} · ${h.records} choices${extraEn}` }));
    const time = Object.assign(document.createElement("time"), { textContent: when(h.t), dateTime: new Date(h.t * 1000).toISOString() });
    const file = Object.assign(document.createElement("span"), { className: "file", textContent: h.file });
    row.append(ic, what, time, file);
    box.append(row);
  }
}

// ---- links open in the default browser (only the few pages the Rust side allows)
document.addEventListener("click", (e) => {
  const a = e.target.closest("[data-link]");
  if (!a) return;
  e.preventDefault();
  invoke("open_url", { url: a.dataset.link }).catch(() => {});
});

// ---- startup
(async () => {
  renderHistory();
  fill(await invoke("get_settings"));
  const a = await invoke("about");
  $("#about-info").innerHTML = "";
  for (const [k, v] of [["الإصدار / Version", a.version], ["الإعدادات / Settings file", a.config_path], ["البيانات / Data", a.data_file]]) {
    const dt = document.createElement("dt");
    dt.textContent = k;
    const dd = document.createElement("dd");
    dd.textContent = v;
    $("#about-info").append(dt, dd);
  }
})();
