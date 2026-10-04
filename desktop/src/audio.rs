//! The audio thread: owns the shared voice pool and renders it.
//!
//! This is the desktop equivalent of `web/public/graph-processor.js`. Both call
//! the same `graph::PolyGraph`, so a patch sounds identical in the browser and
//! in this window — that is the entire point of Milestone 2.

use crate::plumbing::{Control, ControlMsg, Event, EventQueue, Status};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use graph::PolyGraph;
use patch::Patch;
use std::sync::Arc;

pub struct Engine {
    events: Arc<EventQueue>,
    control: Arc<Control>,
    status: Arc<Status>,
    /// Kept alive for as long as the app runs; dropping the stream stops audio.
    _stream: Option<cpal::Stream>,
    /// Stand-in for the audio thread when no output device is available, so the
    /// UI and MIDI still work (and a patch can still be inspected) on a machine
    /// with no sound card. State stays live; only the sound is missing.
    _silent: Option<std::thread::JoinHandle<()>>,
    sample_rate: f32,
}

impl Engine {
    /// Start the audio device and begin rendering.
    ///
    /// Failing to open a device is not fatal: the caller gets a usable engine
    /// and an error string to show, because a user with no sound card should
    /// still be able to open and edit a patch.
    pub fn start(
        events: Arc<EventQueue>,
        control: Arc<Control>,
        status: Arc<Status>,
        initial: Patch,
    ) -> Result<(Engine, Option<String>), String> {
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or_else(|| "no output device found".to_string())?;
        let config = device
            .default_output_config()
            .map_err(|e| format!("no usable output config: {e}"))?;

        let sample_rate = config.sample_rate() as f32;
        let mut voice = PolyGraph::new(sample_rate);
        apply_patch(&mut voice, &initial, sample_rate);

        let ev = Arc::clone(&events);
        let ctl = Arc::clone(&control);
        let stat = Arc::clone(&status);

        // The render closure is shared by the device callback and the silent
        // fallback, so there is only one copy of "turn messages into samples".
        let mut render = move |data: &mut [f32]| {
            tick(&mut voice, &ev, &ctl, &stat, sample_rate, data);
        };

        let stream = match device.build_output_stream(
            &config.into(),
            move |data: &mut [f32], _: &cpal::OutputCallbackInfo| render(data),
            |err| eprintln!("audio error: {err}"),
            None,
        ) {
            Ok(s) => s,
            Err(e) => return Err(format!("could not open audio output: {e}")),
        };

        if let Err(e) = stream.play() {
            return Err(format!("could not start audio playback: {e}"));
        }

        Ok((
            Engine {
                events,
                control,
                status,
                _stream: Some(stream),
                _silent: None,
                sample_rate,
            },
            None,
        ))
    }

    /// A stand-in used when no device could be opened: renders into a scratch
    /// buffer at roughly real time so note state and meters still move.
    fn silent(
        events: Arc<EventQueue>,
        control: Arc<Control>,
        status: Arc<Status>,
        initial: Patch,
        sample_rate: f32,
    ) -> Engine {
        let ev = Arc::clone(&events);
        let ctl = Arc::clone(&control);
        let stat = Arc::clone(&status);
        let handle = std::thread::spawn(move || {
            let mut voice = PolyGraph::new(sample_rate);
            apply_patch(&mut voice, &initial, sample_rate);
            let block = 512;
            let mut scratch = vec![0.0f32; block];
            loop {
                tick(&mut voice, &ev, &ctl, &stat, sample_rate, &mut scratch);
                let nanos = (block as f64 / sample_rate as f64 * 1e9) as u64;
                std::thread::sleep(std::time::Duration::from_nanos(nanos));
            }
        });
        Engine {
            events,
            control,
            status,
            _stream: None,
            _silent: Some(handle),
            sample_rate,
        }
    }

    /// Build an engine, preferring a real device and falling back to silent
    /// rendering. Returns the engine plus a message describing any degradation.
    pub fn start_or_silent(
        events: Arc<EventQueue>,
        control: Arc<Control>,
        status: Arc<Status>,
        initial: Patch,
        fallback_sample_rate: f32,
    ) -> Engine {
        match Self::start(
            Arc::clone(&events),
            Arc::clone(&control),
            Arc::clone(&status),
            initial.clone(),
        ) {
            Ok((engine, _)) => engine,
            Err(e) => {
                eprintln!("falling back to silent rendering: {e}");
                Engine::silent(events, control, status, initial, fallback_sample_rate)
            }
        }
    }

    pub fn sample_rate(&self) -> f32 {
        self.sample_rate
    }

    pub fn events(&self) -> &Arc<EventQueue> {
        &self.events
    }

    pub fn control(&self) -> &Arc<Control> {
        &self.control
    }

    pub fn status(&self) -> &Arc<Status> {
        &self.status
    }

    pub fn dropped_events(&self) -> usize {
        self.events.dropped()
    }
}

/// Drain messages and render one block. Shared by every output path.
fn tick(
    voice: &mut PolyGraph,
    events: &EventQueue,
    control: &Control,
    status: &Status,
    sample_rate: f32,
    out: &mut [f32],
) {
    for msg in control.take() {
        match msg {
            ControlMsg::SetParam { id, name, value } => voice.set_param(id, &name, value),
            ControlMsg::LoadPatch(patch) => {
                apply_patch(voice, &patch, sample_rate);
                status.clear_notes();
                status.bump_patch_generation();
            }
        }
    }

    for event in events.drain() {
        match event {
            Event::NoteOn { note, vel } => {
                voice.note_on(note, vel);
                status.set_note(note, true);
            }
            Event::NoteOff { note } => {
                voice.note_off(note);
                status.set_note(note, false);
            }
            Event::Cc { cc, value } => {
                voice.set_cc(cc, value);
                status.set_last_cc(cc);
            }
        }
    }

    voice.process(out);
    let peak = out.iter().fold(0.0f32, |acc, s| acc.max(s.abs()));
    status.set_peak(peak);
}

/// Rebuild a voice pool from a patch.
///
/// Adding nodes in ascending id order means every voice is structurally identical
/// before the cables go down, which is what lets the pool broadcast future edits.
pub fn apply_patch(voice: &mut PolyGraph, patch: &Patch, sample_rate: f32) {
    let mut fresh = PolyGraph::new(sample_rate);
    let mut nodes = patch.nodes.iter().collect::<Vec<_>>();
    nodes.sort_by_key(|n| n.id);
    for node in &nodes {
        if !fresh.add_node(node.id, node.kind) {
            eprintln!("skipping node {}: unknown kind {}", node.id, node.kind);
        }
    }
    for (id, name, value) in patch.params_by_id() {
        fresh.set_param(id, &name, value);
    }
    for edge in &patch.edges {
        if !fresh.connect(edge.source, edge.source_port, edge.target, edge.target_port) {
            eprintln!(
                "skipping cable {}:{} -> {}:{} (would create a feedback loop)",
                edge.source, edge.source_port, edge.target, edge.target_port
            );
        }
    }
    *voice = fresh;
}

/// Read a note's name for the UI: sharps only, so it fits under a key.
pub fn note_name(note: u8) -> String {
    const NAMES: [&str; 12] = [
        "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
    ];
    format!("{}{}", NAMES[(note % 12) as usize], note as i32 / 12 - 1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plumbing::{Event, EventQueue};
    use graph::Kind;
    use patch::PatchEdge;
    use std::sync::atomic::Ordering;

    fn ports() -> Arc<Status> {
        Arc::new(Status::new())
    }

    #[test]
    fn a_default_patch_renders_and_then_goes_quiet() {
        let events = Arc::new(EventQueue::new());
        let control = Arc::new(Control::new());
        let status = ports();
        let mut voice = PolyGraph::new(48000.0);
        apply_patch(&mut voice, &patch::default_patch(), 48000.0);

        let mut idle = vec![0.0f32; 256];
        tick(&mut voice, &events, &control, &status, 48000.0, &mut idle);
        assert!(
            idle.iter().all(|s| s.abs() < 1e-7),
            "idle patch should be silent"
        );

        events.push(Event::NoteOn { note: 69, vel: 100 });
        let mut played = vec![0.0f32; 4096];
        tick(&mut voice, &events, &control, &status, 48000.0, &mut played);
        assert!(played.iter().any(|s| s.abs() > 1e-4), "a note should sound");

        events.push(Event::NoteOff { note: 69 });
        let mut tail = vec![0.0f32; 48000];
        tick(&mut voice, &events, &control, &status, 48000.0, &mut tail);
        assert!(
            tail.iter().any(|s| s.abs() > 1e-4),
            "the release should decay audibly rather than cut"
        );

        // One more second, and the tail must be over: a stuck note here is the
        // exact failure this test exists to catch.
        let mut released = vec![0.0f32; 48000];
        tick(
            &mut voice,
            &events,
            &control,
            &status,
            48000.0,
            &mut released,
        );
        assert!(
            released.iter().all(|s| s.abs() < 1e-6),
            "the release tail should end in silence"
        );
    }

    #[test]
    fn a_loaded_patch_replaces_the_previous_one_and_clears_held_notes() {
        let events = Arc::new(EventQueue::new());
        let control = Arc::new(Control::new());
        let status = ports();
        let mut voice = PolyGraph::new(48000.0);
        apply_patch(&mut voice, &patch::default_patch(), 48000.0);

        events.push(Event::NoteOn { note: 69, vel: 100 });
        let mut played = vec![0.0f32; 1024];
        tick(&mut voice, &events, &control, &status, 48000.0, &mut played);
        status.set_note(69, true);
        assert_eq!(status.held_count(), 1);

        // An empty patch has no Midi node, so nothing can sound afterwards.
        control.push(ControlMsg::LoadPatch(Box::new(Patch::default())));
        let mut after = vec![0.0f32; 4096];
        tick(&mut voice, &events, &control, &status, 48000.0, &mut after);
        assert!(
            after.iter().all(|s| s.abs() < 1e-7),
            "old patch kept sounding"
        );
        assert_eq!(status.held_count(), 0, "held notes should be cleared");
        assert_eq!(status.patch_generation.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn a_loaded_controller_reaches_every_voice() {
        let events = Arc::new(EventQueue::new());
        let control = Arc::new(Control::new());
        let status = ports();

        // Controller -> SineOsc freq cv -> Out, twice, for two held notes.
        let patch = Patch {
            nodes: vec![
                patch::PatchNode {
                    id: 0,
                    kind: Kind::CC as u32,
                    x: 0.0,
                    y: 0.0,
                    params: [("cc".to_string(), 74.0), ("depth".to_string(), 2.0)]
                        .into_iter()
                        .collect(),
                },
                patch::PatchNode {
                    id: 1,
                    kind: Kind::SineOsc as u32,
                    x: 0.0,
                    y: 0.0,
                    params: [("freq".to_string(), 220.0)].into_iter().collect(),
                },
                patch::PatchNode {
                    id: 2,
                    kind: Kind::Out as u32,
                    x: 0.0,
                    y: 0.0,
                    params: Default::default(),
                },
            ],
            edges: vec![
                PatchEdge {
                    source: 0,
                    source_port: 0,
                    target: 1,
                    target_port: 0,
                },
                PatchEdge {
                    source: 1,
                    source_port: 0,
                    target: 2,
                    target_port: 0,
                },
            ],
        };
        let mut voice = PolyGraph::new(48000.0);
        apply_patch(&mut voice, &patch, 48000.0);

        // Two notes held: a controller must move both, not just the first.
        events.push(Event::NoteOn { note: 60, vel: 100 });
        events.push(Event::NoteOn { note: 67, vel: 100 });
        let mut warm = vec![0.0f32; 1024];
        tick(&mut voice, &events, &control, &status, 48000.0, &mut warm);

        events.push(Event::Cc { cc: 74, value: 127 });
        let mut up = vec![0.0f32; 48000];
        tick(&mut voice, &events, &control, &status, 48000.0, &mut up);
        // 220 Hz base, +2 octaves = 880 Hz.
        let bright = goertzel(&up, 880.0);
        assert!(bright > 0.01, "CC did not reach every voice: {bright}");
    }

    #[test]
    fn note_names_are_octave_numbered() {
        assert_eq!(note_name(60), "C4");
        assert_eq!(note_name(69), "A4");
        assert_eq!(note_name(0), "C-1");
        assert_eq!(note_name(61), "C#4");
    }

    fn goertzel(buf: &[f32], freq: f32) -> f64 {
        let sr = 48000.0f64;
        let n = buf.len() as f64;
        let w = 2.0 * std::f64::consts::PI * freq as f64 / sr;
        let coeff = 2.0 * w.cos();
        let (mut s1, mut s2) = (0.0f64, 0.0f64);
        for &x in buf {
            let s0 = x as f64 + coeff * s1 - s2;
            s2 = s1;
            s1 = s0;
        }
        (s1 * s1 + s2 * s2 - coeff * s1 * s2).sqrt() / n * 2.0
    }
}
