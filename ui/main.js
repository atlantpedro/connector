// The window: open documents, messages, settings. Talks to the app through the commands in
// app/src/commands.rs and refreshes on its events.

const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

const TEXTS = {
  en: {
    tabDocuments: "Documents",
    tabMessages: "Messages",
    tabSettings: "Settings",
    noDocuments: "No document is open.",
    noDocumentsHint: "Click “Bearbeiten” in the Datenportal to open a document.",
    pendingTitle: "Waiting from earlier",
    pendingHint: "These documents were closed, but the server hasn't taken everything yet. Datenportal Connector keeps trying in the background.",
    openFolder: "Open folder",
    openLogs: "Open log folder",
    noMessages: "No messages yet.",
    noMessagesHint: "Anything Datenportal Connector has to tell you is kept here.",
    languageAuto: "Automatic",
    languageSystem: "as the system: {lang}",
    editorTitle: "Edit documents in",
    editorHint: "Applies to documents opened from now on.",
    promptsLabel: "Don't ask “Keep current format?” on every save, and hide the bar about disabled macros",
    promptsHint: "LibreOffice has to be closed for this.",
    promptsNoProfile: "Start LibreOffice once first; it has no settings yet.",
    backgroundTitle: "In the background",
    autostartLabel: "Start at login",
    autostartHint: "Finishes uploads that a shutdown interrupted. Recommended.",
    trayLabel: "Show an icon in the system tray",
    trayHint: "Keeps Datenportal Connector running, with your documents and messages one click away.",
    trayLibrary: "Not available: the libayatana-appindicator library is not installed.",
    trayGnome: "GNOME shows tray icons only with the AppIndicator extension.",
    aboutTitle: "About",
    updatesTitle: "Updates",
    updatesAutoLabel: "Look for updates once a day",
    updateCheck: "Look now",
    updateLooking: "Looking…",
    updateCurrent: "Datenportal Connector is up to date.",
    updateAvailable: "Version {version} is available.",
    updateInstall: "Install and restart",
    updateInstalling: "Installing…",
    folderIs: "Versions that could not be uploaded are kept in",
    installed: "installed",
    notInstalled: "not installed",
    phase_opening: "Opening",
    phase_editing: "Open in {editor}",
    phase_uploading: "Uploading",
    phase_waiting: "Waiting for the server",
    phase_closing: "Finishing",
    phase_conflict: "Changed by someone else",
    finishing: "Finishing after “Done editing”",
    lastUpload: "uploaded {time}",
    noteWaiting: "Your saves are not in the CMS yet. Datenportal Connector keeps trying; leave the computer on until it reports success.",
    noteConflict: "Someone else changed or locked the document in the CMS. Your saves are no longer uploaded; your version is kept when you close it.",
    doneEditing: "Done editing",
    doneConfirm: "Upload the last save and release the document in the CMS? Anything you save after this won't reach the CMS.",
    doneYes: "Release document",
    cancel: "Cancel",
    pendingUnsynced: "Changes not in the CMS yet",
    pendingUnlock: "Release in the CMS pending",
    since: "since {time}",
    setAside: "Stop trying…",
    setAsideConfirm: "Stop trying to finish this document? Your version goes to your folder. If the CMS still shows it as checked out, an admin has to unlock it.",
    setAsideYes: "Stop trying",
    setAsideBusy: "It is being worked on right now; try again in a moment.",
    keptOpen: "Open",
    keptShow: "Show in folder",
    today: "today",
    yesterday: "yesterday",
  },
  de: {
    tabDocuments: "Dokumente",
    tabMessages: "Meldungen",
    tabSettings: "Einstellungen",
    noDocuments: "Kein Dokument geöffnet.",
    noDocumentsHint: "Klicken Sie im Datenportal auf „Bearbeiten“, um ein Dokument zu öffnen.",
    pendingTitle: "Noch offen von früher",
    pendingHint: "Diese Dokumente wurden geschlossen, aber der Server hat noch nicht alles übernommen. Datenportal Connector versucht es im Hintergrund weiter.",
    openFolder: "Ordner öffnen",
    openLogs: "Protokollordner öffnen",
    noMessages: "Noch keine Meldungen.",
    noMessagesHint: "Alles, was Datenportal Connector Ihnen mitteilt, wird hier aufbewahrt.",
    languageAuto: "Automatisch",
    languageSystem: "wie das System: {lang}",
    editorTitle: "Dokumente bearbeiten mit",
    editorHint: "Gilt für Dokumente, die ab jetzt geöffnet werden.",
    promptsLabel: "Nicht bei jedem Speichern „Aktuelles Format beibehalten?“ fragen und die Leiste zu deaktivierten Makros ausblenden",
    promptsHint: "Dafür muss LibreOffice geschlossen sein.",
    promptsNoProfile: "Starten Sie LibreOffice zuerst einmal; es hat noch keine Einstellungen.",
    backgroundTitle: "Im Hintergrund",
    autostartLabel: "Bei der Anmeldung starten",
    autostartHint: "Schließt Uploads ab, die ein Herunterfahren unterbrochen hat. Empfohlen.",
    trayLabel: "Symbol im Infobereich anzeigen",
    trayHint: "Datenportal Connector läuft dann weiter, Dokumente und Meldungen sind einen Klick entfernt.",
    trayLibrary: "Nicht verfügbar: Die Bibliothek libayatana-appindicator ist nicht installiert.",
    trayGnome: "GNOME zeigt solche Symbole nur mit der Erweiterung „AppIndicator“.",
    aboutTitle: "Über",
    updatesTitle: "Updates",
    updatesAutoLabel: "Einmal am Tag nach Updates suchen",
    updateCheck: "Jetzt suchen",
    updateLooking: "Wird gesucht…",
    updateCurrent: "Datenportal Connector ist auf dem neuesten Stand.",
    updateAvailable: "Version {version} ist verfügbar.",
    updateInstall: "Installieren und neu starten",
    updateInstalling: "Wird installiert…",
    folderIs: "Fassungen, die nicht hochgeladen werden konnten, liegen in",
    installed: "installiert",
    notInstalled: "nicht installiert",
    phase_opening: "Wird geöffnet",
    phase_editing: "Geöffnet in {editor}",
    phase_uploading: "Wird hochgeladen",
    phase_waiting: "Wartet auf den Server",
    phase_closing: "Wird abgeschlossen",
    phase_conflict: "Von jemand anderem geändert",
    finishing: "Wird nach „Fertig bearbeitet“ abgeschlossen",
    lastUpload: "hochgeladen {time}",
    noteWaiting: "Ihre Speicherungen sind noch nicht im CMS. Datenportal Connector versucht es weiter; lassen Sie den Computer eingeschaltet, bis er Erfolg meldet.",
    noteConflict: "Jemand anderes hat das Dokument im CMS geändert oder gesperrt. Ihre Speicherungen werden nicht mehr hochgeladen; Ihre Fassung wird beim Schließen aufbewahrt.",
    doneEditing: "Fertig bearbeitet",
    doneConfirm: "Die letzte Speicherung hochladen und das Dokument im CMS freigeben? Was Sie danach speichern, kommt nicht mehr ins CMS.",
    doneYes: "Dokument freigeben",
    cancel: "Abbrechen",
    pendingUnsynced: "Änderungen noch nicht im CMS",
    pendingUnlock: "Freigabe im CMS steht aus",
    since: "seit {time}",
    setAside: "Nicht weiter versuchen…",
    setAsideConfirm: "Nicht mehr versuchen, dieses Dokument abzuschließen? Ihre Fassung kommt in Ihren Ordner. Wenn das CMS es noch als ausgecheckt zeigt, muss ein Administrator die Sperre aufheben.",
    setAsideYes: "Nicht weiter versuchen",
    setAsideBusy: "Daran wird gerade gearbeitet; versuchen Sie es gleich noch einmal.",
    keptOpen: "Öffnen",
    keptShow: "Im Ordner zeigen",
    today: "heute",
    yesterday: "gestern",
  },
};

let lang = "en";
let view = null;
/** Keys of cards whose confirmation is open; kept across refreshes. */
const confirming = new Set();

function tr(key, vars = {}) {
  let text = (TEXTS[lang] || TEXTS.en)[key] ?? TEXTS.en[key] ?? key;
  for (const [k, v] of Object.entries(vars)) text = text.replace(`{${k}}`, v);
  return text;
}

function el(tag, attrs = {}, ...children) {
  const node = document.createElement(tag);
  for (const [k, v] of Object.entries(attrs)) {
    if (v == null || v === false) continue;
    if (k === "class") node.className = v;
    else if (k.startsWith("on")) node.addEventListener(k.slice(2), v);
    else node.setAttribute(k, v === true ? "" : v);
  }
  for (const child of children.flat()) {
    if (child != null && child !== false) node.append(child);
  }
  return node;
}

const SVG = "http://www.w3.org/2000/svg";
function icon(kind) {
  const paths = {
    doc: ["M9 3h10l6 6v20H9z", "M19 3v6h6M13 16h8M13 20h8M13 24h5"],
    info: ["M10 1.8a8.2 8.2 0 1 0 0 16.4 8.2 8.2 0 0 0 0-16.4z", "M10 9v5M10 6.2v.1"],
    warn: ["M10 2.2 1.6 17h16.8z", "M10 8v4.5M10 14.8v.1"],
  }[kind];
  const box = kind === "doc" ? "0 0 34 32" : "0 0 20 20";
  const svg = document.createElementNS(SVG, "svg");
  svg.setAttribute("viewBox", box);
  svg.setAttribute("aria-hidden", "true");
  for (const d of paths) {
    const path = document.createElementNS(SVG, "path");
    path.setAttribute("d", d);
    path.setAttribute("fill", "none");
    path.setAttribute("stroke", "currentColor");
    path.setAttribute("stroke-width", kind === "doc" ? "2" : "1.8");
    path.setAttribute("stroke-linecap", "round");
    path.setAttribute("stroke-linejoin", "round");
    svg.append(path);
  }
  return svg;
}

function formatTime(iso) {
  if (!iso) return "";
  const date = new Date(iso);
  const locale = lang === "de" ? "de-DE" : "en-GB";
  const time = date.toLocaleTimeString(locale, { hour: "2-digit", minute: "2-digit" });
  const day = new Date(date); day.setHours(0, 0, 0, 0);
  const today = new Date(); today.setHours(0, 0, 0, 0);
  const days = Math.round((today - day) / 86400000);
  if (days === 0) return `${tr("today")} ${time}`;
  if (days === 1) return `${tr("yesterday")} ${time}`;
  const sameYear = date.getFullYear() === new Date().getFullYear();
  const dateText = date.toLocaleDateString(locale, { day: "numeric", month: "short", year: sameYear ? undefined : "numeric" });
  return `${dateText}, ${time}`;
}

function applyTexts() {
  document.documentElement.lang = lang;
  for (const node of document.querySelectorAll("[data-t]")) node.textContent = tr(node.dataset.t);
}

// --- views ---

function show(name) {
  if (!["sessions", "messages", "settings"].includes(name)) name = "sessions";
  view = name;
  for (const tab of document.querySelectorAll(".tabs button")) {
    tab.setAttribute("aria-selected", String(tab.dataset.view === name));
  }
  for (const section of document.querySelectorAll(".view")) section.hidden = section.id !== `view-${name}`;
  if (history.replaceState) history.replaceState(null, "", `#${name}`);
  if (name === "messages") loadMessages();
  if (name === "settings") loadSettings();
}

// --- documents ---

async function loadOverview() {
  const overview = await invoke("overview");
  if (overview.lang !== lang) {
    lang = overview.lang;
    applyTexts();
  }
  setUnread(overview.unread);
  renderSessions(overview.sessions, overview.pending);
}

function setUnread(count) {
  const badge = document.getElementById("unread");
  badge.hidden = count === 0 || view === "messages";
  badge.textContent = count > 99 ? "99+" : String(count);
}

function confirmBox(key, text, yesLabel, yesClass, onYes) {
  const cancel = () => { confirming.delete(key); loadOverview(); };
  return el("div", { class: "confirm" },
    el("p", {}, text),
    el("div", { class: "row" },
      el("button", { onclick: cancel }, tr("cancel")),
      el("button", { class: yesClass, onclick: onYes }, yesLabel)));
}

function renderSessions(sessions, pending) {
  const list = document.getElementById("sessions-list");
  list.replaceChildren(...sessions.map(sessionCard));
  document.getElementById("sessions-empty").hidden = sessions.length > 0;
  document.getElementById("pending").hidden = pending.length === 0;
  document.getElementById("pending-list").replaceChildren(...pending.map(pendingCard));
  const keys = new Set([...sessions, ...pending].map((s) => s.key));
  for (const key of confirming) if (!keys.has(key)) confirming.delete(key);
}

function sessionCard(s) {
  const attention = s.phase === "waiting";
  const conflict = s.phase === "conflict";
  const chip = s.finishing && !attention && !conflict
    ? el("span", { class: "chip closing" }, tr("finishing"))
    : el("span", { class: `chip ${s.phase}` }, tr(`phase_${s.phase}`, { editor: s.editor }));
  const meta = el("div", { class: "meta" }, chip, s.uploaded && el("span", {}, tr("lastUpload", { time: formatTime(s.uploaded) })));
  const confirmKey = `done:${s.key}`;
  const canFinish = !s.finishing && !conflict && s.phase !== "opening";
  const actions = el("div", { class: "actions" },
    canFinish && !confirming.has(confirmKey) && el("button", {
      onclick: () => { confirming.add(confirmKey); loadOverview(); },
    }, tr("doneEditing")));
  const card = el("article", { class: `card${attention ? " attention" : ""}${conflict ? " conflict" : ""}` },
    el("div", { class: "icon" }, icon("doc")),
    el("div", { class: "name" }, s.name),
    actions,
    meta,
    attention && el("p", { class: "note" }, tr("noteWaiting")),
    conflict && el("p", { class: "note" }, tr("noteConflict")),
    canFinish && confirming.has(confirmKey) && confirmBox(confirmKey, tr("doneConfirm"), tr("doneYes"), "primary", async () => {
      confirming.delete(confirmKey);
      await invoke("finish_session", { key: s.key });
      loadOverview();
    }));
  return card;
}

function pendingCard(p) {
  const confirmKey = `aside:${p.key}`;
  const error = el("p", { class: "error note", hidden: true });
  const meta = el("div", { class: "meta" },
    el("span", { class: "chip waiting" }, tr(p.unsynced ? "pendingUnsynced" : "pendingUnlock")),
    p.since && el("span", {}, tr("since", { time: formatTime(p.since) })));
  return el("article", { class: "card attention" },
    el("div", { class: "icon" }, icon("doc")),
    el("div", { class: "name" }, p.name),
    el("div", { class: "actions" },
      !confirming.has(confirmKey) && el("button", {
        onclick: () => { confirming.add(confirmKey); loadOverview(); },
      }, tr("setAside"))),
    meta,
    error,
    confirming.has(confirmKey) && confirmBox(confirmKey, tr("setAsideConfirm"), tr("setAsideYes"), "danger", async (event) => {
      event.target.disabled = true;
      try {
        const done = await invoke("set_aside", { key: p.key });
        confirming.delete(confirmKey);
        if (!done) {
          error.textContent = tr("setAsideBusy");
          error.hidden = false;
          return;
        }
      } catch (e) {
        confirming.delete(confirmKey);
        error.textContent = String(e);
        error.hidden = false;
        return;
      }
      loadOverview();
    }));
}

// --- messages ---

async function loadMessages() {
  const { messages, seen } = await invoke("messages");
  const list = document.getElementById("messages-list");
  list.replaceChildren(...messages.map((m) => messageItem(m, m.id > seen)));
  list.hidden = messages.length === 0;
  document.getElementById("messages-empty").hidden = messages.length > 0;
  if (messages.length && messages[0].id > seen && view === "messages") {
    await invoke("mark_seen", { id: messages[0].id });
  }
  setUnread(0);
}

function messageItem(m, isNew) {
  const error = el("p", { class: "error", hidden: true });
  const run = (command) => async () => {
    try {
      await invoke(command, { path: m.kept });
      error.hidden = true;
    } catch (e) {
      error.textContent = String(e);
      error.hidden = false;
    }
  };
  return el("div", { class: `message${m.urgent ? " urgent" : ""}${isNew ? " new" : ""}` },
    el("div", { class: "kind" }, icon(m.urgent ? "warn" : "info")),
    el("div", { class: "time" }, formatTime(m.time)),
    el("p", { class: "text" }, m.text),
    m.kept && el("div", { class: "kept" },
      el("button", { onclick: run("open_kept") }, tr("keptOpen")),
      el("button", { onclick: run("show_kept") }, tr("keptShow")),
      error));
}

// --- settings ---

async function loadSettings() {
  const s = await invoke("settings");
  document.getElementById("version").textContent = s.version;
  document.getElementById("folder").textContent = s.folder;

  renderLanguages(s.language, s.system_lang);
  renderUpdates(s);

  const editors = document.getElementById("editors");
  editors.replaceChildren(...s.editors.map((e) => {
    const chosen = e.id === s.editor;
    const input = el("input", { type: "radio", name: "editor", value: e.id, checked: chosen, disabled: !e.installed && !chosen });
    input.addEventListener("change", () => save("set_editor", { editor: e.id }));
    return el("label", { class: `option${input.disabled ? " disabled" : ""}` }, input,
      el("span", {}, el("span", {}, e.name), el("span", { class: "hint" }, tr(e.installed ? "installed" : "notInstalled"))));
  }));

  const prompts = document.getElementById("prompts");
  prompts.checked = s.prompts_off;
  prompts.disabled = !s.prompts_available && !s.prompts_off;
  prompts.closest("label").classList.toggle("disabled", prompts.disabled);
  document.getElementById("prompts-hint").textContent = prompts.disabled ? tr("promptsNoProfile") : tr("promptsHint");

  document.getElementById("autostart").checked = s.autostart;

  const tray = document.getElementById("tray");
  tray.checked = s.tray;
  tray.disabled = !s.tray_available;
  tray.closest("label").classList.toggle("disabled", tray.disabled);
  const trayNote = { library: "trayLibrary", gnome: "trayGnome" }[s.tray_note] ?? "trayHint";
  document.getElementById("tray-hint").textContent = tr(trayNote);
}

function renderUpdates(s) {
  document.getElementById("updates").hidden = !s.updates_supported;
  document.getElementById("updates-auto").checked = s.updates_auto;
  showUpdate(s.update_available ?? undefined); // "up to date" only right after looking
}

/** version: the one available, null if up to date, undefined if not looked for yet. */
function showUpdate(version) {
  const install = document.getElementById("update-install");
  install.hidden = !version;
  install.textContent = tr("updateInstall");
  install.disabled = false;
  const status = document.getElementById("update-status");
  status.textContent = version ? tr("updateAvailable", { version }) : version === null ? tr("updateCurrent") : "";
}

function wireUpdates() {
  const error = document.getElementById("update-error");
  const fail = (e) => { error.textContent = String(e); error.hidden = false; };
  const auto = document.getElementById("updates-auto");
  auto.addEventListener("change", async () => {
    if (!(await save("set_updates", { on: auto.checked }, "update-error"))) auto.checked = !auto.checked;
  });
  const check = document.getElementById("update-check");
  check.addEventListener("click", async () => {
    error.hidden = true;
    check.disabled = true;
    document.getElementById("update-status").textContent = tr("updateLooking");
    try {
      showUpdate(await invoke("check_update"));
    } catch (e) {
      showUpdate(undefined);
      fail(e);
    }
    check.disabled = false;
  });
  const install = document.getElementById("update-install");
  install.addEventListener("click", async () => {
    error.hidden = true;
    install.disabled = true;
    install.textContent = tr("updateInstalling");
    try {
      await invoke("install_update"); // the app restarts
    } catch (e) {
      install.disabled = false;
      install.textContent = tr("updateInstall");
      fail(e);
    }
  });
}

/** Language names are always in their own language. */
const LANGUAGE_NAMES = { de: "Deutsch", en: "English" };

function renderLanguages(chosen, system) {
  const options = [[null, tr("languageAuto"), tr("languageSystem", { lang: LANGUAGE_NAMES[system] })], ["de", "Deutsch"], ["en", "English"]];
  document.getElementById("languages").replaceChildren(...options.map(([id, name, hint]) => {
    const input = el("input", { type: "radio", name: "language", value: id ?? "auto", checked: id === (chosen ?? null) });
    input.addEventListener("change", async () => {
      try {
        lang = await invoke("set_language", { language: id });
        applyTexts();
        await loadOverview();
        await loadSettings();
      } catch (e) {
        const error = document.getElementById("language-error");
        error.textContent = String(e);
        error.hidden = false;
      }
    });
    return el("label", { class: "option" }, input, el("span", {}, el("span", {}, name), hint && el("span", { class: "hint" }, hint)));
  }));
}

async function save(command, args, errorId = "settings-error") {
  const error = document.getElementById(errorId);
  try {
    await invoke(command, args);
    error.hidden = true;
    return true;
  } catch (e) {
    error.textContent = String(e);
    error.hidden = false;
    return false;
  }
}

function wireSettings() {
  const prompts = document.getElementById("prompts");
  prompts.addEventListener("change", async () => {
    prompts.disabled = true;
    const ok = await save("set_prompts", { off: prompts.checked }, "prompts-error");
    if (!ok) prompts.checked = !prompts.checked;
    prompts.disabled = false;
  });
  for (const [id, command] of [["autostart", "set_autostart"], ["tray", "set_tray"]]) {
    const box = document.getElementById(id);
    box.addEventListener("change", async () => {
      if (!(await save(command, { on: box.checked }))) box.checked = !box.checked;
    });
  }
}

// --- start ---

function debounce(fn, ms) {
  let timer = null;
  return () => { clearTimeout(timer); timer = setTimeout(fn, ms); };
}

async function start() {
  for (const tab of document.querySelectorAll(".tabs button")) {
    tab.addEventListener("click", () => show(tab.dataset.view));
  }
  for (const button of document.querySelectorAll("[data-action]")) {
    const command = { "open-folder": "open_folder", "open-logs": "open_log_folder" }[button.dataset.action];
    button.addEventListener("click", () => invoke(command).catch(() => {}));
  }
  wireSettings();
  wireUpdates();
  await loadOverview();
  applyTexts();
  show(location.hash.slice(1));

  await listen("sessions-changed", debounce(loadOverview, 100));
  await listen("messages-changed", debounce(() => {
    loadOverview();
    if (view === "messages") loadMessages();
  }, 100));
  await listen("navigate", (event) => show(event.payload));
  await listen("update-changed", () => { if (view === "settings") loadSettings(); });
  // relative times ("today") stay right overnight
  setInterval(() => { if (view === "sessions") loadOverview(); }, 60000);
}

start();
