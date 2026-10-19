const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;
const { getCurrentWindow } = window.__TAURI__.window;

const $ = (id) => document.getElementById(id);
const listEl = $("list");

// Enabled state as Claude last loaded it; differences from this need a restart.
let baseline = null;
let items = [];
let dismissed = false;

/** Config servers and extensions as one list of {key, kind, id, name, enabled, detail}. */
function toItems(listing) {
  return [
    ...listing.servers.map((s) => ({ ...s, kind: "server", id: s.name, key: `server:${s.name}` })),
    ...listing.extensions.map((e) => ({ ...e, kind: "extension", key: `extension:${e.id}` })),
  ];
}

function changedKeys() {
  if (!baseline) return new Set();
  return new Set(items.filter((i) => baseline.has(i.key) && baseline.get(i.key) !== i.enabled).map((i) => i.key));
}

function showError(msg) {
  const el = $("error");
  el.textContent = msg || "";
  el.hidden = !msg;
}

function sectionHeader(text) {
  const h = document.createElement("div");
  h.className = "section";
  h.textContent = text;
  return h;
}

function renderRow(item, changed) {
  const row = document.createElement("div");
  row.className = "row" + (item.enabled ? "" : " off") + (changed ? " changed" : "");
  row.tabIndex = 0;
  row.title = changed ? "Changed — restart Claude to apply" : item.detail;

  const text = document.createElement("div");
  text.className = "row-text";
  const name = document.createElement("div");
  name.className = "row-name";
  name.textContent = item.name;
  const detail = document.createElement("div");
  detail.className = item.kind === "extension" ? "row-detail plain" : "row-detail";
  detail.textContent = item.detail || "—";
  text.append(name, detail);

  const dot = document.createElement("span");
  dot.className = "dot";

  const toggle = document.createElement("span");
  toggle.className = "toggle";
  toggle.setAttribute("role", "switch");
  toggle.setAttribute("aria-checked", String(item.enabled));
  toggle.setAttribute("aria-label", `${item.name} enabled`);

  row.append(text, dot, toggle);
  const flip = () => setEnabled(item, !item.enabled);
  row.addEventListener("click", flip);
  row.addEventListener("keydown", (e) => {
    if (e.key === " " || e.key === "Enter") { e.preventDefault(); flip(); }
  });
  return row;
}

function render(listing) {
  items = toItems(listing);
  if (!baseline) baseline = new Map();
  for (const i of items) if (!baseline.has(i.key)) baseline.set(i.key, i.enabled);

  $("path").textContent = listing.config_path;
  $("path").title = listing.config_path;

  listEl.replaceChildren();
  const servers = items.filter((i) => i.kind === "server");
  const extensions = items.filter((i) => i.kind === "extension");
  const changed = changedKeys();

  if (!listing.config_found && extensions.length === 0) {
    listEl.innerHTML = `<div class="empty">No <code>claude_desktop_config.json</code> found.<br/>Is Claude Desktop installed?</div>`;
  } else if (items.length === 0) {
    listEl.innerHTML = `<div class="empty">No MCP servers configured.</div>`;
  }

  if (servers.length && extensions.length) listEl.append(sectionHeader("Config servers"));
  for (const i of servers) listEl.append(renderRow(i, changed.has(i.key)));
  if (extensions.length) listEl.append(sectionHeader("Extensions"));
  for (const i of extensions) listEl.append(renderRow(i, changed.has(i.key)));

  const n = changed.size;
  if (n === 0) dismissed = false;
  $("pending").hidden = n === 0 || dismissed;
  $("pending-count").textContent = `${n} server${n === 1 ? "" : "s"} changed`;
}

async function load() {
  try {
    render(await invoke("list_servers"));
    showError("");
  } catch (e) {
    showError(String(e));
  }
}

async function setEnabled(item, enabled) {
  try {
    dismissed = false;
    const listing =
      item.kind === "extension"
        ? await invoke("set_extension_enabled", { id: item.id, enabled })
        : await invoke("set_server_enabled", { name: item.id, enabled });
    render(listing);
    showError("");
  } catch (e) {
    showError(String(e));
  }
}

$("refresh").addEventListener("click", () => {
  const b = $("refresh");
  b.classList.remove("spin");
  void b.offsetWidth;
  b.classList.add("spin");
  load();
});

$("later").addEventListener("click", () => {
  dismissed = true;
  $("pending").hidden = true;
});

$("restart").addEventListener("click", async () => {
  const btn = $("restart");
  btn.disabled = true;
  btn.textContent = "Restarting…";
  try {
    await invoke("restart_claude");
    baseline = new Map(items.map((i) => [i.key, i.enabled]));
    showError("");
    await load();
    getCurrentWindow().hide();
  } catch (e) {
    showError(String(e));
  } finally {
    btn.disabled = false;
    btn.textContent = "Restart now";
  }
});

$("open-folder").addEventListener("click", () => invoke("open_config_folder").catch((e) => showError(String(e))));

document.addEventListener("keydown", (e) => {
  if (e.key === "Escape") getCurrentWindow().hide();
});

listen("refresh", load);
load();
