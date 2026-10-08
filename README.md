# Datenportal Connector

**Deutsch** · [English](#english)

Dokumente aus dem Datenportal in LibreOffice (oder Microsoft Office) bearbeiten,
unter Linux, macOS und Windows: Ein Klick auf „Bearbeiten“ im CMS öffnet das Dokument, und
jedes Speichern landet direkt wieder im CMS.

Der CMS-Button erzeugt Office-Links (`ms-word:ofe|u|https://…/webdav/…`). LibreOffice kann
diese selbst öffnen, aber der LibreOffice WebDAV-Client scheitert ab dem zweiten
Speichern. Datenportal Connector übernimmt deshalb WebDAV selbst, und das Office-Programm
bearbeitet immer nur eine lokale Kopie. 

## Installation

Die Installationspakete stehen unter
[Releases](https://github.com/atlantpedro/connector/releases).

| System | Paket | Updates |
|---|---|---|
| Windows | `Datenportal Connector_<Version>_x64-setup.exe` (ohne Administratorrechte) | in der App |
| macOS | `Datenportal Connector_<Version>_universal.dmg` | in der App |
| Linux | `.AppImage` | in der App |
| Linux | `.deb` | über die Paketverwaltung |
| Linux | `datenportal-connector-<Architektur>.flatpak` | über Flatpak |

Nach der Installation ist Datenportal Connector das Programm für `ms-word:`- und
`ms-excel:`-Links. Unter Linux und macOS geschieht das beim ersten Start; starten Sie die App
also einmal aus dem Menü. Die Deinstallation unter Windows gibt die Links an das Programm
zurück, das sie vorher hatte; unter Linux tut das `datenportal-connector --uninstall`.

- **Windows:** Hat sich Windows für diese Links schon ein anderes Programm gemerkt, kann ein
  Installationsprogramm das nicht ändern. Es öffnet dann **Einstellungen > Apps >
  Standard-Apps**; legen Sie dort *Datenportal Connector* für `MS-WORD` und `MS-EXCEL` fest.
- **macOS:** Die App ist nicht von Apple beglaubigt. Öffnen Sie sie beim ersten Mal mit
  Rechtsklick > Öffnen.
- **Linux:** Ein Browser aus Flatpak oder Snap sieht die Programme des Systems
  möglicherweise nicht; nehmen Sie dann einen nativ installierten Browser.

## Benutzung

- **Dokument schließen, wenn Sie fertig sind.** LibreOffice selbst müssen Sie nicht beenden.
  Das Dokument gilt als geschlossen, sobald die Sperrdatei des Office-Programms
  (`.~lock.<Name>#` bzw. `~$…`) verschwunden ist. Wird das einmal nicht erkannt, klicken Sie
  im Fenster auf **Fertig bearbeitet**.
- **Das Fenster** öffnen Sie, indem Sie Datenportal Connector aus dem Menü starten:
  - *Dokumente:* geöffnete Dokumente und ihr Zustand; darunter frühere Sitzungen, die noch auf
    den Server warten. Lehnt der Server eine davon dauerhaft ab, beenden Sie sie mit
    **Nicht weiter versuchen**; Ihre Fassung kommt dann in den Ordner (siehe unten).
  - *Meldungen:* alles, was die App Ihnen mitgeteilt hat. Dringende Meldungen öffnen das
    Fenster von selbst.
  - *Einstellungen:* Sprache, Office-Programm, die Format-Rückfrage und Makro-Leiste von
    LibreOffice, Start bei der Anmeldung, Symbol im Infobereich, Updates.
- Fragt LibreOffice beim Speichern nach dem Format, wählen Sie das **Word-Format**. Eine als
  ODF gespeicherte Datei wird nicht hochgeladen; die App warnt und verschiebt sie danach in
  ihren Ordner. Die Rückfrage lässt sich in den Einstellungen abschalten.
- **Ohne Server** (offline) lädt die App im Hintergrund hoch, sobald es wieder geht, auch nach
  dem Schließen des Dokuments und nach einem Neustart. Lassen Sie den Computer eingeschaltet,
  bis sie Erfolg meldet.
- Hat in der Zwischenzeit jemand anderes das Dokument im CMS geändert oder gesperrt, lädt die
  App nichts mehr hoch und bewahrt Ihre Fassung als
  `<Name> (nicht hochgeladen <Zeitpunkt>).<Endung>` im Ordner
  `Dokumente/Datenportal Connector` auf.
- Nach dem Hochladen löscht die App ihre Arbeitskopie. Öffnen Sie CMS-Dokumente deshalb immer
  aus dem CMS, nicht über „Zuletzt benutzte Dokumente“.

## Dateien

| | Linux | macOS | Windows |
|---|---|---|---|
| aufbewahrte Dokumente | `Dokumente/Datenportal Connector/` | dto. | dto. |
| Arbeitskopien | `~/.cache/datenportal-connector/` | `~/Library/Caches/datenportal-connector/` | `%LOCALAPPDATA%\datenportal-connector\cache\` |
| Meldungen, Einstellungen, Protokoll, `sessions/` | `~/.local/state/datenportal-connector/` | `~/Library/Application Support/datenportal-connector/` | `%LOCALAPPDATA%\datenportal-connector\state\` |

## Haftungsauschluss, Support

Diese Software ist eine inoffizielle Hilfssoftware. Sie wurde gründlich und erfolgreich erprobt, es kann aber keine Haftung übernommen und kein Support angeboten werden. Benutzung auf eigene Verantwortung und eigene Gefahr.

## Lizenz

MIT, siehe [LICENSE](LICENSE).

---

<a id="english"></a>

# Datenportal Connector (English)

Edit documents from Datenportal in LibreOffice (or Microsoft Office) on Linux,
macOS and Windows: click "Bearbeiten" in the CMS and the document opens; every save goes
straight back to the CMS.

The CMS button emits Office links (`ms-word:ofe|u|https://…/webdav/…`). LibreOffice can
open those itself, but its WebDAV client fails from the second save onwards. Datenportal Connector therefore handles WebDAV on its own, and the office program
only ever edits a local copy.

## Install

Packages are on the [releases page](https://github.com/atlantpedro/connector/releases).

| System | Package | Updates |
|---|---|---|
| Windows | `Datenportal Connector_<version>_x64-setup.exe` (no admin rights needed) | in the app |
| macOS | `Datenportal Connector_<version>_universal.dmg` | in the app |
| Linux | `.AppImage` | in the app |
| Linux | `.deb` | through the package manager |
| Linux | `datenportal-connector-<arch>.flatpak` | through Flatpak |

Once installed, Datenportal Connector handles `ms-word:` and `ms-excel:` links. On Linux and
macOS that happens on its first run, so start the app once from the menu. Uninstalling on
Windows hands the links back to the program that had them; on Linux,
`datenportal-connector --uninstall` does.

- **Windows:** if Windows has remembered another program for these links, an installer
  can't change that. It opens **Settings > Apps > Default apps** then; pick *Datenportal
  Connector* for `MS-WORD` and `MS-EXCEL` there.
- **macOS:** the app isn't notarized yet. Open it with right-click > Open the first time.
- **Linux:** a browser installed as a Flatpak or Snap may not see the system's programs; use
  a natively installed browser then.

## Usage

- **Close the document when you're done.** You don't have to quit LibreOffice. The document
  counts as closed once the office program's lock file (`.~lock.<name>#` or `~$…`) is gone.
  If that's ever missed, click **Done editing** in the window.
- **The window** opens when you start Datenportal Connector from the menu:
  - *Documents:* open documents and their state; below them, earlier sessions still waiting
    for the server. If the server keeps refusing one, end it with **Stop trying**; your
    version then goes to the folder (see below).
  - *Messages:* everything the app has told you. Urgent messages open the window by
    themselves.
  - *Settings:* language, office program, LibreOffice's format question and macro bar,
    start at login, tray icon, updates.
- When LibreOffice asks which format to save in, choose the **Word format**. A file saved as
  ODF isn't uploaded; the app warns and moves it to its folder afterwards. The question can
  be turned off in the settings.
- **Without the server** (offline), the app uploads in the background as soon as it can,
  also after the document is closed and after a restart. Leave the computer on until it
  reports success.
- If someone else changed or locked the document in the CMS meanwhile, the app stops
  uploading and keeps your version as `<name> (not uploaded <time>).<ext>` in
  `Documents/Datenportal Connector`.
- After uploading, the app deletes its working copy. Always open CMS documents from the CMS,
  not from "Recent Documents".

## Files

| | Linux | macOS | Windows |
|---|---|---|---|
| kept documents | `Documents/Datenportal Connector/` | same | same |
| working copies | `~/.cache/datenportal-connector/` | `~/Library/Caches/datenportal-connector/` | `%LOCALAPPDATA%\datenportal-connector\cache\` |
| messages, settings, log, `sessions/` | `~/.local/state/datenportal-connector/` | `~/Library/Application Support/datenportal-connector/` | `%LOCALAPPDATA%\datenportal-connector\state\` |

`LODAV_LANG=de` or `LODAV_LANG=en` sets the language while the setting is "Automatic".

## Development

```sh
cargo test --workspace                       # core, with whole sessions against a mock CMS
cargo run -p datenportal-connector           # the app, as a development build
(cd app && cargo tauri build)                # installers for this system
flatpak-builder --user --install --force-clean build flatpak/io.github.atlantpedro.connector.yml
```

Linux needs `webkit2gtk-4.1` and, for the tray icon at runtime, `libayatana-appindicator3`.
Development builds don't take over the links and don't set up a start at login.

`core/` (`lodav-core`) has everything that talks to the CMS and the office program and no
GUI; `app/` is the Tauri app around it; `ui/` is the window, plain HTML/CSS/JS.

**Releasing:** set the version in `app/tauri.conf.json` and `Cargo.toml`, then push a tag
`v<version>`. The release workflow builds everything into a draft release, including the
updater's signed `latest.json`; publish the draft to ship the update. It needs the repository
secrets `TAURI_SIGNING_PRIVATE_KEY` and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` (the updater
key; its public half is in `app/tauri.conf.json`).

## Support

This is an unofficial client, available for free. It has been thorougly and successfully tested, and should work; but there's no support for this and no liability. Use at your own risk.

## License

MIT, see [LICENSE](LICENSE).
