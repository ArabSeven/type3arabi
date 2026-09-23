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

function status(text, kind) {
  const el = $("#status");
  el.textContent = text;
  el.className = kind || "";
}

// ---- shortcut capture
const KEY_NAMES = { " ": "Space", Enter: "Enter", Tab: "Tab", Escape: "Esc", Backspace: "Backspace" };
const MODIFIERS = ["Control", "Alt", "Shift", "Meta"];

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

$$('input[data-kind]').forEach((el) => {
  el.addEventListener("focus", () => {
    el.dataset.before = el.value;
    el.classList.add("capturing");
    el.value = "…اضغط / press";
  });
  el.addEventListener("blur", () => {
    el.classList.remove("capturing");
    if (el.value.startsWith("…")) el.value = el.dataset.before;
  });
  el.addEventListener("keydown", async (e) => {
    e.preventDefault();
    const chord = chordFromEvent(e, el.dataset.kind);
    if (chord === null) return; // lone modifier: keep waiting
    if (chord === undefined) {
      status("هذا المفتاح غير مدعوم. That key is not supported.", "err");
      return;
    }
    const ok = el.dataset.kind === "chord" ? await invoke("check_chord", { chord }) : /(Ctrl|Alt|Win)\+/.test(chord);
    el.value = chord;
    el.classList.toggle("invalid", !ok);
    if (!ok) status("اختصار غير صالح. Invalid shortcut (the global hotkey needs Ctrl, Alt or Win).", "err");
    else status("");
    el.dataset.before = chord;
    el.blur();
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
    status("تم الحفظ — يطبَّق عند الكلمة التالية. Saved — applies from the next word.", "ok");
  } catch (errors) {
    status((Array.isArray(errors) ? errors : [String(errors)]).join("\n"), "err");
  }
});

$("#reset").addEventListener("click", async () => {
  fill(await invoke("defaults"));
  status("القيم الافتراضية — اضغط حفظ لتطبيقها. Defaults loaded — press Save to apply.");
});

$("#wipe").addEventListener("click", async () => {
  if (!confirm("نسيان كل الكلمات والاختيارات المتعلَّمة؟\nForget every learned word and choice?")) return;
  try {
    await invoke("wipe_learning");
    status("تم نسيان كل ما تعلّمه تعريب. Everything learned was forgotten.", "ok");
  } catch (e) {
    status(String(e), "err");
  }
});

// ---- startup
(async () => {
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
