//! Every text the user gets to see, in German and English. The log stays English.

use std::fmt::Display;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lang {
    En,
    De,
}

impl Lang {
    /// The user's language: German if `LODAV_LANG` or else the system locale says so.
    pub fn detect() -> Lang {
        let lang = std::env::var("LODAV_LANG").ok().or_else(sys_locale::get_locale);
        Lang::from_tag(lang.as_deref().unwrap_or(""))
    }

    pub fn from_tag(tag: &str) -> Lang {
        if tag.to_ascii_lowercase().starts_with("de") { Lang::De } else { Lang::En }
    }
}

macro_rules! messages {
    ($($id:ident: $en:literal => $de:literal,)*) => {
        /// A text shown to the user; its `{placeholders}` are filled in by [`Msg::fill`].
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum Msg { $($id,)* }

        impl Msg {
            pub const ALL: &[Msg] = &[$(Msg::$id,)*];

            pub fn text(self, lang: Lang) -> &'static str {
                match (self, lang) {
                    $((Msg::$id, Lang::En) => $en, (Msg::$id, Lang::De) => $de,)*
                }
            }
        }
    };
}

messages! {
    NotUploadedKept:
        "Changes to {name} could not be uploaded. Your version is kept at {path}" =>
        "Die Änderungen an {name} konnten nicht hochgeladen werden. Ihre Fassung liegt unter {path}",
    UploadPending:
        "Changes to {name} could not be uploaded yet. Datenportal Connector keeps trying in the \
         background; leave the computer on until it reports success." =>
        "Die Änderungen an {name} konnten noch nicht hochgeladen werden. Datenportal Connector \
         versucht es im Hintergrund weiter; lassen Sie den Computer eingeschaltet, bis er Erfolg meldet.",
    ClosedPending:
        "{name} was closed, but its changes are not in the CMS yet. Datenportal Connector keeps \
         trying in the background; leave the computer on until it reports success." =>
        "{name} wurde geschlossen, die Änderungen sind aber noch nicht im CMS. Datenportal Connector \
         versucht es im Hintergrund weiter; lassen Sie den Computer eingeschaltet, bis er Erfolg meldet.",
    Uploaded:
        "Changes to {name} have been uploaded to the CMS." =>
        "Die Änderungen an {name} wurden ins CMS hochgeladen.",
    Conflict:
        "{name} was changed or locked in the CMS by someone else while the server could not be \
         reached. Your saves are NOT uploaded any more; your version is kept when you close the \
         document." =>
        "{name} wurde im CMS von jemand anderem geändert oder gesperrt, während der Server nicht \
         erreichbar war. Ihre Speicherungen werden NICHT mehr hochgeladen; Ihre Fassung wird beim \
         Schließen des Dokuments aufbewahrt.",
    EarlierPending:
        "Changes to {name} from an earlier session are not in the CMS yet. Datenportal Connector \
         keeps trying in the background; leave the computer on until it reports success." =>
        "Änderungen an {name} aus einer früheren Sitzung sind noch nicht im CMS. Datenportal \
         Connector versucht es im Hintergrund weiter; lassen Sie den Computer eingeschaltet, bis er \
         Erfolg meldet.",
    EarlierBlocked:
        "Earlier changes to {name} are not in the CMS yet because the server can't be reached. \
         Try again later." =>
        "Frühere Änderungen an {name} sind noch nicht im CMS, weil der Server nicht erreichbar ist. \
         Versuchen Sie es später erneut.",
    OtherFormat:
        "{other} was saved next to {name}, probably in a different format. It is NOT uploaded to \
         the CMS; only {name} is. Open the document from the CMS again and save it in its original \
         format. Your file is kept at {path}" =>
        "{other} wurde neben {name} gespeichert, vermutlich in einem anderen Format. Diese Datei \
         wird NICHT ins CMS hochgeladen, nur {name}. Öffnen Sie das Dokument erneut aus dem CMS und \
         speichern Sie es im ursprünglichen Format. Ihre Datei liegt unter {path}",
    StillLocked:
        "{name} still appears locked on the server; an admin may need to unlock it" =>
        "{name} ist auf dem Server anscheinend noch gesperrt. Möglicherweise muss ein \
         Administrator die Sperre aufheben.",
    LockedReadOnly:
        "{name} is locked (checked out by someone else). Opening read-only." =>
        "{name} ist gesperrt (von jemand anderem ausgecheckt). Das Dokument wird schreibgeschützt \
         geöffnet.",
    CouldNotOpen:
        "Could not open document: {error}" =>
        "Das Dokument konnte nicht geöffnet werden: {error}",
    BadLink:
        "The link from the CMS could not be read." =>
        "Der Link aus dem CMS konnte nicht gelesen werden.",
    ServerUnreachable:
        "The server could not be reached." =>
        "Der Server war nicht erreichbar.",
    ServerRefused:
        "The server refused it (HTTP {status})." =>
        "Der Server hat abgelehnt (HTTP {status}).",
    NoPermission:
        "A file on this computer could not be written: no permission." =>
        "Eine Datei auf diesem Computer konnte nicht geschrieben werden: keine Berechtigung.",
    DiskFull:
        "A file on this computer could not be written: the disk is full." =>
        "Eine Datei auf diesem Computer konnte nicht geschrieben werden: Der Datenträger ist voll.",
    StillBusy:
        "Another document is still being finished. Try again in a moment." =>
        "Ein anderes Dokument wird noch abgeschlossen. Versuchen Sie es gleich noch einmal.",
    LocalFiles:
        "A file on this computer could not be read or written. Details are in the log." =>
        "Eine Datei auf diesem Computer konnte nicht gelesen oder geschrieben werden. Einzelheiten \
         stehen im Protokoll.",
    Failed:
        "That didn't work. Details are in the log." =>
        "Das hat nicht geklappt. Einzelheiten stehen im Protokoll.",
    EditorNotFound:
        "{editor} was not found." =>
        "{editor} wurde nicht gefunden.",
    NoLibreOfficeSettings:
        "No LibreOffice settings found. Start LibreOffice once, then try again." =>
        "Keine LibreOffice-Einstellungen gefunden. Starten Sie LibreOffice einmal und versuchen Sie \
         es dann erneut.",
    LibreOfficeRunning:
        "LibreOffice is running. Close it, then try again." =>
        "LibreOffice läuft noch. Schließen Sie es und versuchen Sie es dann erneut.",
    KeptName:
        "{stem} (not uploaded {time}){suffix}" =>
        "{stem} (nicht hochgeladen {time}){suffix}",
    SetAside:
        "Datenportal Connector stopped trying to finish {name}. If the CMS still shows it as \
         checked out, an admin has to unlock it." =>
        "Datenportal Connector versucht nicht mehr, {name} abzuschließen. Wenn das CMS das Dokument \
         noch als ausgecheckt zeigt, muss ein Administrator die Sperre aufheben.",
    MovedTo:
        "{file} was moved to {path}" =>
        "{file} wurde nach {path} verschoben",
}

impl Msg {
    /// The text in lang with its `{placeholders}` filled in.
    pub fn fill(self, lang: Lang, values: &[(&str, &dyn Display)]) -> String {
        let mut text = self.text(lang).to_string();
        for (key, value) in values {
            text = text.replace(&format!("{{{key}}}"), &value.to_string());
        }
        text
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn placeholders(text: &str) -> Vec<&str> {
        let mut found: Vec<&str> = text.split('{').skip(1).filter_map(|s| s.split_once('}')).map(|(k, _)| k).collect();
        found.sort();
        found.dedup();
        found
    }

    #[test]
    fn german_keeps_placeholders() {
        for msg in Msg::ALL {
            assert_eq!(placeholders(msg.text(Lang::En)), placeholders(msg.text(Lang::De)), "{msg:?}");
        }
    }

    #[test]
    fn fill() {
        let text = Msg::Uploaded.fill(Lang::De, &[("name", &"a.docx")]);
        assert_eq!(text, "Die Änderungen an a.docx wurden ins CMS hochgeladen.");
    }

    #[test]
    fn language_tags() {
        assert_eq!(Lang::from_tag("de-DE"), Lang::De);
        assert_eq!(Lang::from_tag("de_AT.UTF-8"), Lang::De);
        assert_eq!(Lang::from_tag("en-US"), Lang::En);
        assert_eq!(Lang::from_tag(""), Lang::En);
    }
}
