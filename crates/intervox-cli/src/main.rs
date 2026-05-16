//! intervox-cli — verification harness for intervox-core.
//!
//! Lets the core be validated end-to-end (DSP, state, routing, ring buffer,
//! OpenAI event model) before the UI, HAL driver, and websocket exist.
//!
//!   intervox selfcheck
//!   intervox resample --in 48000 --out 24000
//!   intervox mix --original-db -18
//!   intervox ringbuffer
//!   intervox parse-event '{"type":"session.updated"}'

use intervox_core::audio::{level_meter::LevelMeter, mixer, resampler};
use intervox_core::config::{db_to_percent, percent_to_db, Config};
use intervox_core::diagnostics::metrics::LatencyMetrics;
use intervox_core::realtime::events::{build_session_update, parse_server_event};
use intervox_core::state::{AppState, VirtualMicMode};
use intervox_core::virtual_mic::ring_buffer::{
    SharedAudioRingBuffer, SharedRingMap, DEFAULT_SHM_NAME,
};
use intervox_core::{pipeline, AppError};

fn arg(args: &[String], key: &str) -> Option<String> {
    args.iter().position(|a| a == key).and_then(|i| args.get(i + 1).cloned())
}

fn sine(freq: f32, sr: u32, secs: f32) -> Vec<f32> {
    let n = (sr as f32 * secs) as usize;
    (0..n)
        .map(|i| (2.0 * std::f32::consts::PI * freq * i as f32 / sr as f32).sin())
        .collect()
}

fn detect_freq(x: &[f32], sr: u32) -> f32 {
    let zc = x.windows(2).filter(|w| w[0] <= 0.0 && w[1] > 0.0).count();
    zc as f32 * sr as f32 / x.len().max(1) as f32
}

fn peak(x: &[f32]) -> f32 {
    x.iter().fold(0.0f32, |m, &v| m.max(v.abs()))
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let cmd = args.get(1).map(|s| s.as_str()).unwrap_or("selfcheck");
    let code = match cmd {
        "selfcheck" => selfcheck(),
        "resample" => {
            let i: u32 = arg(&args, "--in").and_then(|s| s.parse().ok()).unwrap_or(48000);
            let o: u32 = arg(&args, "--out").and_then(|s| s.parse().ok()).unwrap_or(24000);
            let input = sine(1000.0, i, 1.0);
            let out = resampler::resample(&input, i, o);
            println!(
                "resample {i} -> {o}: in {} samples ({:.0} Hz) -> out {} samples ({:.0} Hz)",
                input.len(),
                detect_freq(&input, i),
                out.len(),
                detect_freq(&out, o)
            );
            0
        }
        "mix" => {
            let odb: f32 = arg(&args, "--original-db")
                .and_then(|s| s.parse().ok())
                .unwrap_or(-18.0);
            let s = mixer::MixSettings {
                original_gain_db: odb,
                ..Default::default()
            };
            let full = vec![1.0f32; 480];
            let zeros = vec![0.0f32; 480];
            let t_only = mixer::mix_frames(&full, &zeros, &s);
            let o_only = mixer::mix_frames(&zeros, &full, &s);
            println!(
                "mix original={odb}dB: translated peak {:.3}, original peak {:.3} ({:.1}%)",
                peak(&t_only),
                peak(&o_only),
                peak(&o_only) * 100.0
            );
            0
        }
        "ringbuffer" => {
            let rb = SharedAudioRingBuffer::new_boxed(48000, 1);
            let data: Vec<f32> = (0..960).map(|i| i as f32 / 960.0).collect();
            let w = rb.write_frames(&data);
            let mut out = vec![0.0; 960];
            let underrun1 = rb.read_into(&mut out);
            let mut empty = vec![9.0; 256];
            let underrun2 = rb.read_into(&mut empty);
            println!(
                "ringbuffer: wrote {w}, read-back match {}, first underrun {underrun1}, empty underrun {underrun2} (silence {})",
                out == data,
                empty.iter().all(|&s| s == 0.0)
            );
            0
        }
        "parse-event" => {
            let raw = args.get(2).cloned().unwrap_or_default();
            println!("{:?}", parse_server_event(&raw));
            0
        }
        "shm-producer" => {
            // Cross-process IPC test: create the same POSIX shm + ring layout
            // the HAL driver consumes, write a known ramp, then hold the
            // mapping alive so a separate consumer process can read it.
            let hold_ms: u64 = arg(&args, "--hold-ms")
                .and_then(|s| s.parse().ok())
                .unwrap_or(3000);
            let tone_hz: Option<f32> = arg(&args, "--tone-hz").and_then(|s| s.parse().ok());
            let secs: f32 = arg(&args, "--secs").and_then(|s| s.parse().ok()).unwrap_or(6.0);
            match SharedRingMap::create(DEFAULT_SHM_NAME, 48000, 1) {
                Ok(map) => {
                    if let Some(hz) = tone_hz {
                        // Continuous 48k mono sine, paced ~real-time in 10 ms
                        // (480-frame) chunks so the HAL driver can read a live
                        // signal end-to-end.
                        println!("producer: created {DEFAULT_SHM_NAME}, prefill+stream {hz}Hz tone for {secs}s");
                        let total = (secs * 48000.0) as u64;
                        let mut phase: u64 = 0;
                        // Keep the ring topped up: write as fast as the
                        // consumer drains it. write_frames returns 0 when
                        // full, so a tiny yield avoids a busy spin while still
                        // never letting the buffer underrun.
                        while phase < total {
                            let chunk: Vec<f32> = (0..480)
                                .map(|i| {
                                    let t = (phase + i as u64) as f32 / 48000.0;
                                    (2.0 * std::f32::consts::PI * hz * t).sin() * 0.5
                                })
                                .collect();
                            let w = map.get().write_frames(&chunk);
                            if w == 0 {
                                std::thread::sleep(std::time::Duration::from_millis(2));
                            } else {
                                phase += w as u64;
                            }
                        }
                        // Hold the mapping so the consumer can drain the tail.
                        std::thread::sleep(std::time::Duration::from_secs_f32(secs.min(10.0)));
                        0
                    } else {
                        let ramp: Vec<f32> = (0..480).map(|i| i as f32 / 480.0).collect();
                        let n = map.get().write_frames(&ramp);
                        println!("producer: created {DEFAULT_SHM_NAME}, wrote {n}, holding {hold_ms}ms");
                        std::thread::sleep(std::time::Duration::from_millis(hold_ms));
                        0
                    }
                }
                Err(e) => {
                    eprintln!("producer: create failed: {e}");
                    1
                }
            }
        }
        "shm-consumer" => match SharedRingMap::open(DEFAULT_SHM_NAME) {
            Ok(map) => {
                let expected: Vec<f32> = (0..480).map(|i| i as f32 / 480.0).collect();
                let mut out = vec![0.0; 480];
                let underrun = map.get().read_into(&mut out);
                let ok = out == expected;
                println!(
                    "consumer: opened {DEFAULT_SHM_NAME}, sample_rate={}, match={ok}, underrun={underrun}",
                    map.get().sample_rate
                );
                if ok && !underrun {
                    0
                } else {
                    1
                }
            }
            Err(e) => {
                eprintln!("consumer: open failed (is producer running?): {e}");
                1
            }
        },
        other => {
            eprintln!("unknown command: {other}");
            2
        }
    };
    std::process::exit(code);
}

/// Runs invariant assertions across every core module. Exit 0 = all PASS.
fn selfcheck() -> i32 {
    let mut pass = 0u32;
    let mut fail = 0u32;
    macro_rules! check {
        ($name:expr, $cond:expr) => {{
            if $cond {
                pass += 1;
                println!("PASS  {}", $name);
            } else {
                fail += 1;
                println!("FAIL  {}", $name);
            }
        }};
    }

    // Config / dB math
    let c = Config::default();
    check!("config default version 1", c.version == 1);
    check!("config mix default 15%", c.mix.original_voice_percent == 15);
    check!(
        "percent<->db round trip",
        (db_to_percent(percent_to_db(15.0)) - 15.0).abs() < 0.02
    );

    // State machine
    let mut st = AppState::new();
    st.transition(VirtualMicMode::Translate);
    check!(
        "translate needs openai",
        VirtualMicMode::Translate.requires_openai()
    );
    check!(
        "passthrough no openai",
        !VirtualMicMode::PassThrough.requires_openai()
    );

    // Pipeline non-negotiable rules (§19)
    let sil = pipeline::route(VirtualMicMode::Silence);
    check!("silence: vmic silent + no openai", sil.vmic_silence && !sil.openai_connected);
    let pt = pipeline::route(VirtualMicMode::PassThrough);
    check!("passthrough: no openai cost", !pt.openai_connected);
    let tr = pipeline::route(VirtualMicMode::Translate);
    check!("translate: no original leak", !tr.mic_to_vmic && !tr.mix_original);

    // Resampler
    let s = sine(1000.0, 48000, 1.0);
    let ds = resampler::resample(&s, 48000, 24000);
    check!("resample halves count", (ds.len() as i64 - 24000).abs() <= 2);
    check!(
        "resample preserves 1kHz",
        (detect_freq(&ds, 24000) - 1000.0).abs() < 30.0
    );

    // Mixer
    let mset = mixer::MixSettings::default();
    let o_only = mixer::mix_frames(&vec![0.0; 256], &vec![1.0; 256], &mset);
    let t_only = mixer::mix_frames(&vec![1.0; 256], &vec![0.0; 256], &mset);
    check!("original quieter than translated", peak(&o_only) < peak(&t_only));
    let limited = mixer::mix_frames(&vec![5.0; 64], &[], &mixer::MixSettings::default());
    check!("limiter caps below full scale", limited.iter().all(|v| v.abs() <= 1.0));

    // Level meter
    let lvl = LevelMeter::measure(&s);
    check!("meter rms ~0.707 for sine", (lvl.rms - 0.707).abs() < 0.02);

    // Ring buffer
    let rb = SharedAudioRingBuffer::new_boxed(48000, 1);
    let payload: Vec<f32> = (0..1000).map(|i| i as f32).collect();
    rb.write_frames(&payload);
    let mut back = vec![0.0; 1000];
    let ur = rb.read_into(&mut back);
    check!("ringbuffer round trip", !ur && back == payload);
    let mut e = vec![1.0; 64];
    check!(
        "ringbuffer underrun -> silence",
        rb.read_into(&mut e) && e.iter().all(|&x| x == 0.0)
    );

    // OpenAI event model
    check!(
        "session.update spec 8.3",
        build_session_update("zh", "en")["session"]["audio"]["output"]["language"] == "en"
    );
    check!(
        "parse session.updated",
        matches!(
            parse_server_event(r#"{"type":"session.updated"}"#),
            intervox_core::realtime::events::TranslationEvent::SessionUpdated
        )
    );

    // Metrics
    let mut m = LatencyMetrics {
        capture_to_send_ms: 30,
        openai_first_audio_ms: 900,
        jitter_buffer_ms: 120,
        virtual_mic_output_lag_ms: 20,
        total_estimated_latency_ms: 0,
    };
    m.recompute_total();
    check!("latency display", m.display_seconds() == "1.1s");

    // Error contract
    let err = AppError::mic_permission_denied();
    check!(
        "error recovery command",
        err.recovery_action.unwrap().command == "open_system_mic_permission_settings"
    );

    println!("\n{} passed, {} failed", pass, fail);
    if fail == 0 {
        0
    } else {
        1
    }
}
