//! Plain-text report of a guided test, in French or English. It is written next to the log
//! and printed by `ardoise.exe --diagnostic --simulated`.

use std::fmt::Write as _;

use crate::diagnostic::{DiagnosticReport, StepId, StepStatus};

struct Labels {
    title: &'static str,
    started: &'static str,
    link: &'static str,
    log: &'static str,
    cancelled: &'static str,
    passed: &'static str,
    failed: &'static str,
    skipped: &'static str,
    summary: fn(usize, usize, usize) -> String,
    step: fn(StepId) -> &'static str,
    detail: fn(&str) -> Option<&'static str>,
    value: fn(&str) -> Option<&'static str>,
}

const FRENCH: Labels = Labels {
    title: "test de la tablette",
    started: "Début",
    link: "Liaison",
    log: "Journal",
    cancelled: "Test arrêté avant la fin.",
    passed: "RÉUSSI",
    failed: "ÉCHEC",
    skipped: "IGNORÉ",
    summary: |p, f, s| format!("Résumé : {p} réussie(s), {f} échec(s), {s} ignorée(s)"),
    step: |step| match step {
        StepId::Adapter => "Adaptateur Bluetooth",
        StepId::ReferencePages => "Pages de référence",
        StepId::Find => "Recherche de la tablette",
        StepId::Connect => "Connexion",
        StepId::Info => "Informations",
        StepId::Sync => "Téléchargement des pages",
        StepId::Live => "Mode direct",
        StepId::Reconnect => "Reconnexion",
    },
    detail: |key| {
        Some(match key {
            "adapter" => "adaptateur",
            "name" => "nom",
            "address" => "adresse",
            "signal" => "signal",
            "registerMode" => "mode enregistrement",
            "mode" => "mode",
            "identity" => "identité",
            "firmware" => "micrologiciel",
            "battery" => "batterie",
            "size" => "taille",
            "pages" => "pages",
            "clockDrift" => "écart d'horloge",
            "strokes" => "traits",
            "points" => "points",
            "deleted" => "effacées",
            "folder" => "dossier",
            "samples" => "points reçus",
            "rate" => "fréquence",
            "xRange" => "plage x",
            "yRange" => "plage y",
            "pressureMax" => "pression max",
            "proximity" => "proximité (entrées / sorties)",
            "reconnections" => "reconnexions",
            "result" => "résultat",
            "error" => "erreur",
            "charging" => "en charge",
            _ => return None,
        })
    },
    value: |key| {
        Some(match key {
            "simulatedAdapter" => "aucun (tablette simulée)",
            "existingRegistration" => "enregistrement existant",
            "registeredNow" => "enregistrée à l'instant",
            "registeredAgain" => "enregistrée de nouveau",
            "savedInDeviceJson" => "enregistrée dans _ardoise/device.json",
            "yes" => "oui",
            "no" => "non",
            "reconnected" => "reconnectée avec l'identité enregistrée",
            "noTabletFound" => "aucune tablette trouvée",
            "needsPreviousStep" => "impossible sans l'étape précédente",
            "notConnected" => "tablette non connectée",
            "noPenData" => "aucun point reçu du stylet",
            "missingReferencePages" => "moins de pages que les pages de référence demandées",
            "pointsOutOfRange" => "points hors de la zone de dessin",
            "clockNotSet" => "l'horloge de la tablette n'a pas pu être réglée",
            "registrationRefused" => "identité refusée et tablette non vue en mode enregistrement",
            "noPagesFolder" => {
                "impossible d'enregistrer les pages (dossier des diagnostics inaccessible) : rien n'a été effacé"
            }
            _ => return None,
        })
    },
};

const ENGLISH: Labels = Labels {
    title: "tablet test",
    started: "Started",
    link: "Link",
    log: "Log",
    cancelled: "Test stopped before the end.",
    passed: "PASSED",
    failed: "FAILED",
    skipped: "SKIPPED",
    summary: |p, f, s| format!("Summary: {p} passed, {f} failed, {s} skipped"),
    step: |step| match step {
        StepId::Adapter => "Bluetooth adapter",
        StepId::ReferencePages => "Reference pages",
        StepId::Find => "Finding the tablet",
        StepId::Connect => "Connection",
        StepId::Info => "Information",
        StepId::Sync => "Page download",
        StepId::Live => "Live mode",
        StepId::Reconnect => "Reconnection",
    },
    detail: |key| {
        Some(match key {
            "adapter" => "adapter",
            "name" => "name",
            "address" => "address",
            "signal" => "signal",
            "registerMode" => "registration mode",
            "mode" => "mode",
            "identity" => "identity",
            "firmware" => "firmware",
            "battery" => "battery",
            "size" => "size",
            "pages" => "pages",
            "clockDrift" => "clock drift",
            "strokes" => "strokes",
            "points" => "points",
            "deleted" => "deleted",
            "folder" => "folder",
            "samples" => "samples",
            "rate" => "rate",
            "xRange" => "x range",
            "yRange" => "y range",
            "pressureMax" => "max pressure",
            "proximity" => "proximity (in / out)",
            "reconnections" => "reconnections",
            "result" => "result",
            "error" => "error",
            "charging" => "charging",
            _ => return None,
        })
    },
    value: |key| {
        Some(match key {
            "simulatedAdapter" => "none needed (simulated tablet)",
            "existingRegistration" => "existing registration",
            "registeredNow" => "registered now",
            "registeredAgain" => "registered again",
            "savedInDeviceJson" => "saved in _ardoise/device.json",
            "yes" => "yes",
            "no" => "no",
            "reconnected" => "connected again with the saved identity",
            "noTabletFound" => "no tablet found",
            "needsPreviousStep" => "needs the previous step",
            "notConnected" => "tablet not connected",
            "noPenData" => "no pen data received",
            "missingReferencePages" => "fewer pages than the reference pages asked for",
            "pointsOutOfRange" => "points outside the drawing area",
            "clockNotSet" => "the tablet clock could not be set",
            "registrationRefused" => "identity refused and tablet not seen in registration mode",
            "noPagesFolder" => {
                "cannot save the pages (diagnostics folder unavailable): nothing was deleted"
            }
            _ => return None,
        })
    },
};

/// Renders the report in `language` (`"fr"`; anything else gives English).
pub fn render(report: &DiagnosticReport, language: &str) -> String {
    let labels = if language.starts_with("fr") {
        &FRENCH
    } else {
        &ENGLISH
    };
    let mut text = String::new();
    let _ = writeln!(text, "Ardoise {} — {}", report.app_version, labels.title);
    let _ = writeln!(text, "{} : {}", labels.started, report.started);
    let _ = writeln!(text, "{} : {}", labels.link, report.transport);
    if let Some(path) = &report.log_path {
        let _ = writeln!(text, "{} : {}", labels.log, path.display());
    }
    text.push('\n');
    for outcome in &report.steps {
        let status = match outcome.status {
            StepStatus::Passed => labels.passed,
            StepStatus::Failed => labels.failed,
            StepStatus::Skipped => labels.skipped,
        };
        let _ = writeln!(text, "[{status}] {}", (labels.step)(outcome.step));
        for detail in &outcome.details {
            let key = (labels.detail)(&detail.key).unwrap_or(detail.key.as_str());
            let value = match detail.value.strip_prefix('@') {
                Some(key) => (labels.value)(key).unwrap_or(key).to_owned(),
                None => detail.value.clone(),
            };
            let _ = writeln!(text, "    {key} : {value}");
        }
    }
    text.push('\n');
    if report.cancelled {
        let _ = writeln!(text, "{}", labels.cancelled);
    }
    let _ = writeln!(
        text,
        "{}",
        (labels.summary)(
            report.count(StepStatus::Passed),
            report.failures(),
            report.count(StepStatus::Skipped)
        )
    );
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostic::{Detail, StepOutcome};

    const KEYS: [&str; 26] = [
        "adapter",
        "name",
        "address",
        "signal",
        "registerMode",
        "mode",
        "identity",
        "firmware",
        "battery",
        "size",
        "pages",
        "clockDrift",
        "strokes",
        "points",
        "deleted",
        "folder",
        "samples",
        "rate",
        "xRange",
        "yRange",
        "pressureMax",
        "proximity",
        "reconnections",
        "result",
        "error",
        "charging",
    ];

    /// Every `@key` value the runner writes, found in its source.
    fn runner_values() -> Vec<String> {
        let source = include_str!("runner.rs");
        source
            .split("\"@")
            .skip(1)
            .filter_map(|rest| rest.split('"').next())
            .map(str::to_owned)
            .collect()
    }

    /// The interface shows the same keys and values: its catalogs must know them all.
    #[test]
    fn the_interface_translates_every_detail_key_and_value() {
        for (language, catalog) in [
            ("fr", include_str!("../../../app/ui/src/locales/fr.json")),
            ("en", include_str!("../../../app/ui/src/locales/en.json")),
        ] {
            let catalog: serde_json::Value = serde_json::from_str(catalog).unwrap();
            for key in KEYS {
                assert!(
                    catalog["diagnostic"]["details"][key].is_string(),
                    "{language}: diagnostic.details.{key}"
                );
            }
            for value in runner_values() {
                assert!(
                    catalog["diagnostic"]["values"][value.as_str()].is_string(),
                    "{language}: diagnostic.values.{value}"
                );
            }
        }
    }

    #[test]
    fn every_value_written_by_the_runner_is_translated_in_both_languages() {
        let values = runner_values();
        assert!(values.len() > 10, "{values:?}");
        for value in values {
            assert!((FRENCH.value)(&value).is_some(), "fr: {value}");
            assert!((ENGLISH.value)(&value).is_some(), "en: {value}");
        }
    }

    #[test]
    fn every_detail_key_is_translated_in_both_languages() {
        for key in KEYS {
            assert!((FRENCH.detail)(key).is_some(), "fr: {key}");
            assert!((ENGLISH.detail)(key).is_some(), "en: {key}");
        }
    }

    #[test]
    fn renders_steps_details_and_summary() {
        let report = DiagnosticReport {
            app_version: "0.1.0".into(),
            transport: "sim".into(),
            started: "2026-10-24T14:03:12+02:00".into(),
            steps: vec![
                StepOutcome {
                    step: StepId::Info,
                    status: StepStatus::Passed,
                    details: vec![Detail {
                        key: "battery".into(),
                        value: "87 %".into(),
                    }],
                },
                StepOutcome {
                    step: StepId::Live,
                    status: StepStatus::Skipped,
                    details: vec![],
                },
            ],
            log_path: None,
            report_path: None,
            cancelled: false,
        };
        let fr = render(&report, "fr");
        assert!(
            fr.contains("[RÉUSSI] Informations\n    batterie : 87 %"),
            "{fr}"
        );
        assert!(fr.contains("[IGNORÉ] Mode direct"), "{fr}");
        assert!(
            fr.ends_with("Résumé : 1 réussie(s), 0 échec(s), 1 ignorée(s)\n"),
            "{fr}"
        );
        let en = render(&report, "en");
        assert!(en.contains("[PASSED] Information"), "{en}");
        assert!(
            en.ends_with("Summary: 1 passed, 0 failed, 1 skipped\n"),
            "{en}"
        );
    }
}
