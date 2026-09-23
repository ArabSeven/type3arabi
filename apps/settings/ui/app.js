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
      status("هذا المفتاح غير مدعوم — جرّب غيره أو اضغط Esc. That key is not supported — try another, or press Esc.", "err");
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
      status(`«${chord}» غير صالح — جرّب اختصاراً آخر أو Esc للإلغاء. "${chord}" can't be used (${why}) — try another, or press Esc.`, "err");
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
