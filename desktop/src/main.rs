//! Standalone Coarse desktop synth (Milestone 2).
//!
//! This window runs the same `graph::PolyGraph` the browser runs, driven by the
//! same `#p=` patch links. Nothing here re-implements the synth: the desktop app
//! owns an audio device and a UI, and the shared crate owns the instrument.
//!
//! Usage:
//!   coarse                      start with the default patch
//!   coarse "#p=eyJ…"            start from a share code or url
//!   coarse patch.json            load a patch file

mod app;
mod audio;
mod midi;
mod plumbing;

use audio::Engine;
use patch::Patch;
use plumbing::{Control, EventQueue, Status};
use std::sync::Arc;

/// Used when no device could be queried, so the app still starts.
const FALLBACK_SAMPLE_RATE: f32 = 48000.0;

fn main() -> eframe::Result<()> {
    let arg = std::env::args().nth(1);
    let (initial, link, notice, warnings) = load_starting_patch(arg.as_deref());

    let events = Arc::new(EventQueue::new());
    let control = Arc::new(Control::new());
    let status = Arc::new(Status::new());

    let engine = Arc::new(Engine::start_or_silent(
        Arc::clone(&events),
        Arc::clone(&control),
        Arc::clone(&status),
        initial.clone(),
        FALLBACK_SAMPLE_RATE,
    ));

    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([900.0, 720.0])
            .with_title("Coarse"),
        ..Default::default()
    };

    eframe::run_native(
        "Coarse",
        native_options,
        Box::new(move |_cc| {
            Ok(Box::new(app::CoarseApp::new(
                Arc::clone(&engine),
                initial,
                warnings,
                link,
                notice,
            )))
        }),
    )
    .map(|_| ())
}

/// Starting patch, plus what to show the user and anything repaired on the way.
type Startup = (Patch, String, Option<String>, Vec<String>);

/// Resolve the patch to start with, given the first command line argument.
///
/// Takes the argument rather than reading the environment, so the path a shared
/// link actually travels is testable: accepting a bare code, a `#p=` url, a `?p=`
/// url and a saved json file is the whole contract between the web app's share
/// button and this window, and none of it should depend on being launched by
/// hand.
fn load_starting_patch(arg: Option<&str>) -> Startup {
    let fallback = |notice: Option<String>, warnings: Vec<String>| {
        let patch = patch::default_patch();
        let link = patch::encode(&patch);
        (patch, link, notice, warnings)
    };

    let Some(arg) = arg else {
        return fallback(None, Vec::new());
    };

    if arg.ends_with(".json") {
        return match std::fs::read_to_string(arg) {
            Ok(text) => {
                let decoded = patch::from_json(&text);
                let link = patch::encode(&decoded.patch);
                (
                    decoded.patch,
                    link,
                    Some(format!("loaded {arg}")),
                    decoded.warnings,
                )
            }
            Err(e) => fallback(Some(format!("could not read {arg}: {e}")), Vec::new()),
        };
    }

    let Some(code) = patch::code_from_link(arg) else {
        return fallback(
            Some("that does not look like a patch link".into()),
            Vec::new(),
        );
    };
    let decoded = patch::decode(code);
    if decoded.patch.nodes.is_empty() {
        // Report what actually went wrong rather than one generic message: a
        // truncated paste and a patch with nothing playable in it are different
        // mistakes, and the decoder already knows which one this is.
        let notice = decoded
            .warnings
            .first()
            .cloned()
            .unwrap_or_else(|| "link had nothing playable in it".into());
        return fallback(Some(notice), decoded.warnings);
    }
    let link = patch::encode(&decoded.patch);
    (
        decoded.patch,
        link,
        Some("loaded patch from link".into()),
        decoded.warnings,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A link captured verbatim from the web app's share button, mapping CC 74 to
    /// an oscillator's frequency. This is what a user pastes into the desktop
    /// window's link field, and it must arrive already wired.
    const SHARED_LINK: &str = "eyJ2IjoxLCJuIjpbWzAsMTMsMCwwLHsiY2MiOjc0LCJkZXB0aCI6Mn1dLFsxLDEwLDI0MCwwLHsiZnJlcSI6MjIwfV1dLCJlIjpbWzAsMCwxLDBdXX0";

    #[test]
    fn with_no_argument_it_starts_on_the_default_patch() {
        let (patch, link, notice, warnings) = load_starting_patch(None);
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(notice, None);
        assert_eq!(patch, patch::default_patch());
        // The share code it shows must be one that reloades to the same patch.
        assert_eq!(patch::decode(&link).patch, patch);
    }

    #[test]
    fn a_link_pasted_from_the_web_app_arrives_already_wired() {
        for arg in [
            SHARED_LINK.to_string(),
            format!("#p={SHARED_LINK}"),
            format!("https://coarse.example/synth#p={SHARED_LINK}"),
        ] {
            let (patch, link, notice, warnings) = load_starting_patch(Some(&arg));
            assert_eq!(warnings, Vec::<String>::new(), "for {arg}");
            assert_eq!(
                notice.as_deref(),
                Some("loaded patch from link"),
                "for {arg}"
            );
            assert_eq!(patch.nodes.len(), 2, "for {arg}");
            assert_eq!(patch.edges.len(), 1, "for {arg}");
            assert_eq!(patch.nodes[0].kind, 13, "the controller, for {arg}");
            assert_eq!(patch.nodes[0].params["cc"], 74.0, "for {arg}");
            assert_eq!(patch.edges[0].source, 0);
            assert_eq!(patch.edges[0].target, 1);
            // Re-sharing must not rewrite what was shared.
            assert_eq!(link, SHARED_LINK, "re-encoding changed the link");
        }
    }

    #[test]
    fn a_valid_but_empty_link_falls_back_and_says_so() {
        // {"v":1,"n":[],"e":[]}: readable, and nothing in it.
        let (patch, _, notice, _) = load_starting_patch(Some("eyJ2IjoxLCJuIjpbXSwiZSI6W119"));
        assert_eq!(patch, patch::default_patch());
        assert_eq!(notice.as_deref(), Some("link had nothing playable in it"));
    }

    #[test]
    fn a_truncated_paste_is_reported_as_unreadable_rather_than_empty() {
        // Half a share code: the mistake a phone paste actually makes.
        let (patch, _, notice, _) = load_starting_patch(Some("#p=eyJ2IjoxLCJuIjpbWzAsMT"));
        assert_eq!(patch, patch::default_patch());
        let notice = notice.expect("should explain itself");
        assert!(
            notice.starts_with("link is not valid"),
            "should name the real problem, got {notice:?}"
        );
    }

    #[test]
    fn something_that_is_not_a_link_says_so_instead_of_guessing() {
        let (patch, _, notice, _) = load_starting_patch(Some("hello"));
        assert_eq!(patch, patch::default_patch());
        let notice = notice.expect("should explain itself");
        assert!(
            notice.starts_with("link is not valid"),
            "should name the real problem, got {notice:?}"
        );
    }

    #[test]
    fn a_saved_json_file_loads_through_the_same_repairs() {
        let path = std::env::temp_dir().join("coarse-startup-patch.json");
        // Deliberately damaged: an unknown module and a cable to nowhere.
        std::fs::write(
            &path,
            r#"{"v":1,"n":[[0,10,0,0,{"freq":440}],[1,99,0,0,{}]],"e":[[0,0,1,0]]}"#,
        )
        .expect("write temp patch");

        let (patch, _, notice, warnings) =
            load_starting_patch(Some(path.to_str().expect("utf-8 path")));
        std::fs::remove_file(&path).ok();

        assert_eq!(
            notice.as_deref(),
            Some(format!("loaded {}", path.display()).as_str())
        );
        assert_eq!(patch.nodes.len(), 1, "the unknown module should be dropped");
        assert_eq!(patch.edges.len(), 0, "the dangling cable should be dropped");
        assert!(!warnings.is_empty(), "repairs should be reported");
    }

    #[test]
    fn a_missing_file_falls_back_rather_than_failing_to_start() {
        let (patch, _, notice, _) = load_starting_patch(Some("/nowhere/coarse-missing.json"));
        assert_eq!(patch, patch::default_patch());
        let notice = notice.expect("a missing file should be reported");
        assert!(notice.starts_with("could not read"), "{notice}");
    }
}
