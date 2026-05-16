//! Real audio device enumeration via cpal.
//!
//! # Send-safety
//! cpal `Host` and `Device` types are NOT `Send`. This module collects all
//! cpal data into owned plain-data structs (`AudioDevices`) before returning.
//! No cpal type ever crosses an `.await` point.

use cpal::traits::{DeviceTrait, HostTrait};

use crate::commands::{AudioDevices, DeviceInfo};

/// Enumerate all available input and output audio devices using the default
/// cpal host. The system default device appears first in each list; any
/// subsequent device with the same name is de-duplicated. On any cpal error
/// the relevant list is left empty — the function never panics.
pub fn enumerate() -> AudioDevices {
    let host = cpal::default_host();

    let inputs = collect_devices(
        host.default_input_device(),
        host.input_devices(),
    );

    let outputs = collect_devices(
        host.default_output_device(),
        host.output_devices(),
    );

    AudioDevices { inputs, outputs }
}

/// Build a `Vec<DeviceInfo>` from an optional default device and an iterator
/// of all devices. The default (if present and its name resolves without
/// error) appears first; the rest follow in enumeration order, with any
/// duplicate names skipped.
fn collect_devices<I, E>(
    default: Option<cpal::Device>,
    all: Result<I, E>,
) -> Vec<DeviceInfo>
where
    I: Iterator<Item = cpal::Device>,
    E: std::fmt::Debug,
{
    let mut out: Vec<DeviceInfo> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();

    // Insert the system default first.
    if let Some(dev) = default {
        if let Ok(name) = dev.name() {
            seen.insert(name.clone());
            out.push(DeviceInfo {
                id: format!("coreaudio:{name}"),
                name,
            });
        }
    }

    // Append remaining devices, skipping duplicates.
    if let Ok(devices) = all {
        for dev in devices {
            if let Ok(name) = dev.name() {
                if seen.insert(name.clone()) {
                    out.push(DeviceInfo {
                        id: format!("coreaudio:{name}"),
                        name,
                    });
                }
            }
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Non-hardware test: `enumerate()` is a total function — it always
    /// returns an `AudioDevices` value whose fields are `Vec`s (possibly
    /// empty) without panicking.
    #[test]
    fn enumerate_is_total() {
        let devices = enumerate();
        // The fields must be Vecs; both may be empty on headless CI.
        let _inputs: &Vec<DeviceInfo> = &devices.inputs;
        let _outputs: &Vec<DeviceInfo> = &devices.outputs;
    }

    /// Hardware-dependent: assert the lists are non-empty.
    /// Skip on CI / headless runners.
    #[test]
    #[ignore]
    fn enumerate_has_devices_on_real_hardware() {
        let devices = enumerate();
        assert!(
            !devices.inputs.is_empty() || !devices.outputs.is_empty(),
            "Expected at least one audio device on real hardware"
        );
    }

    /// Hardware-dependent: default device appears first when present.
    #[test]
    #[ignore]
    fn default_device_is_first() {
        use cpal::traits::HostTrait;
        let host = cpal::default_host();

        if let Some(default_in) = host.default_input_device() {
            if let Ok(default_name) = default_in.name() {
                let devices = enumerate();
                let first = devices.inputs.first().expect("inputs should be non-empty");
                assert_eq!(first.name, default_name);
            }
        }

        if let Some(default_out) = host.default_output_device() {
            if let Ok(default_name) = default_out.name() {
                let devices = enumerate();
                let first = devices.outputs.first().expect("outputs should be non-empty");
                assert_eq!(first.name, default_name);
            }
        }
    }

    /// Hardware-dependent: no duplicate ids in either list.
    #[test]
    #[ignore]
    fn no_duplicate_ids() {
        let devices = enumerate();
        let mut ids: Vec<&str> = devices.inputs.iter().map(|d| d.id.as_str()).collect();
        ids.sort();
        let before = ids.len();
        ids.dedup();
        assert_eq!(before, ids.len(), "duplicate input device ids");

        let mut ids: Vec<&str> = devices.outputs.iter().map(|d| d.id.as_str()).collect();
        ids.sort();
        let before = ids.len();
        ids.dedup();
        assert_eq!(before, ids.len(), "duplicate output device ids");
    }
}
