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
            "proximity" => "proximité",
            "reconnections" => "reconnexions",
            "result" => "résultat",
            "error" => "erreur",
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
            "proximity" => "proximity",
            "reconnections" => "reconnections",
            "result" => "result",
            "error" => "error",
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
            let _ = writeln!(text, "    {key} : {}", detail.value);
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

    const KEYS: [&str; 25] = [
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
    ];

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
