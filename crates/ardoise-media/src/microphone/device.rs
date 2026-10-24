//! The thin `cpal` layer: listing the input devices and opening a stream whose callback sends
//! converted samples to the recorder's worker. No logic lives here (it is tested by hand on a
//! PC with a microphone; everything else is tested without hardware).

use std::fmt::Display;
use std::sync::mpsc::Sender;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Device, ErrorKind, FromSample, Host, SampleFormat, SizedSample, Stream, StreamConfig};

use super::InputDevice;
use super::pipeline::convert_to_f32;
use super::recorder::{CaptureEvent, CaptureFormat};
use crate::MediaError;

pub(crate) struct OpenedInput {
    pub stream: Stream,
    pub format: CaptureFormat,
    pub name: String,
}

fn failed(step: &'static str, e: impl Display) -> MediaError {
    MediaError::Microphone {
        step,
        message: e.to_string(),
    }
}

/// Maps a `cpal` error that happened while opening or starting a stream.
fn open_failed(step: &'static str, e: cpal::Error) -> MediaError {
    if e.kind() == ErrorKind::PermissionDenied {
        // Same cause and same remedy as a stream of pure zeros: the Windows privacy setting.
        MediaError::MicrophoneSilent
    } else {
        failed(step, e)
    }
}

fn name_of(device: &Device) -> String {
    device
        .description()
        .map(|d| d.name().to_owned())
        .or_else(|_| device.id().map(|id| id.to_string()))
        .unwrap_or_else(|_| "?".to_owned())
}

/// The input devices of the default host (WASAPI on Windows), the default one marked.
pub(crate) fn list() -> Result<Vec<InputDevice>, MediaError> {
    let host = cpal::default_host();
    let default_id = host.default_input_device().and_then(|d| d.id().ok());
    let devices = host
        .input_devices()
        .map_err(|e| failed("listing the microphones", e))?;
    let mut list = Vec::new();
    for device in devices {
        let Ok(id) = device.id() else { continue };
        list.push(InputDevice {
            name: name_of(&device),
            is_default: default_id.as_ref() == Some(&id),
            id: id.to_string(),
        });
    }
    Ok(list)
}

/// Finds a device by `cpal` id first, then by exact name, then by name ignoring case.
fn find(host: &Host, wanted: &str) -> Result<Device, MediaError> {
    if let Ok(id) = wanted.parse::<cpal::DeviceId>()
        && let Some(device) = host.device_by_id(&id)
        && device.supports_input()
    {
        return Ok(device);
    }
    let devices: Vec<Device> = host
        .input_devices()
        .map_err(|e| failed("listing the microphones", e))?
        .collect();
    devices
        .iter()
        .find(|d| name_of(d) == wanted)
        .or_else(|| {
            devices
                .iter()
                .find(|d| name_of(d).eq_ignore_ascii_case(wanted))
        })
        .cloned()
        .ok_or_else(|| MediaError::MicrophoneNotFound(wanted.to_owned()))
}

/// Opens and starts an input stream on the chosen device (the default if `None`), in the
/// device's own default format.
pub(crate) fn open_input(
    selector: Option<&str>,
    tx: Sender<CaptureEvent>,
) -> Result<OpenedInput, MediaError> {
    let host = cpal::default_host();
    let device = match selector {
        Some(wanted) => find(&host, wanted)?,
        None => host
            .default_input_device()
            .ok_or_else(|| failed("opening the microphone", "this PC has no microphone"))?,
    };
    let supported = device
        .default_input_config()
        .map_err(|e| open_failed("reading the microphone format", e))?;
    let config: StreamConfig = supported.into();
    let format = CaptureFormat {
        sample_rate: config.sample_rate,
        channels: config.channels,
    };

    let stream = match supported.sample_format() {
        SampleFormat::I8 => build::<i8>(&device, config, tx),
        SampleFormat::I16 => build::<i16>(&device, config, tx),
        SampleFormat::I24 => build::<cpal::I24>(&device, config, tx),
        SampleFormat::I32 => build::<i32>(&device, config, tx),
        SampleFormat::I64 => build::<i64>(&device, config, tx),
        SampleFormat::U8 => build::<u8>(&device, config, tx),
        SampleFormat::U16 => build::<u16>(&device, config, tx),
        SampleFormat::U24 => build::<cpal::U24>(&device, config, tx),
        SampleFormat::U32 => build::<u32>(&device, config, tx),
        SampleFormat::U64 => build::<u64>(&device, config, tx),
        SampleFormat::F32 => build::<f32>(&device, config, tx),
        SampleFormat::F64 => build::<f64>(&device, config, tx),
        other => {
            return Err(failed(
                "opening the microphone",
                format!("unsupported sample format {other}"),
            ));
        }
    }
    .map_err(|e| open_failed("opening the microphone", e))?;
    stream
        .play()
        .map_err(|e| open_failed("starting the microphone", e))?;

    Ok(OpenedInput {
        stream,
        format,
        name: name_of(&device),
    })
}

fn build<T>(
    device: &Device,
    config: StreamConfig,
    tx: Sender<CaptureEvent>,
) -> Result<Stream, cpal::Error>
where
    T: SizedSample + 'static,
    f32: FromSample<T>,
{
    let error_tx = tx.clone();
    device.build_input_stream(
        config,
        move |data: &[T], _| {
            // Light work only: convert and hand over; the worker does the rest. A closed
            // channel means the recording is over.
            let mut samples = Vec::with_capacity(data.len());
            convert_to_f32(data, &mut samples);
            let _ = tx.send(CaptureEvent::Samples(samples));
        },
        move |e| {
            // Glitches (xruns), a re-route to another default device and a refused real-time
            // priority leave the stream running; the rest ends it.
            if matches!(
                e.kind(),
                ErrorKind::DeviceNotAvailable
                    | ErrorKind::StreamInvalidated
                    | ErrorKind::HostUnavailable
            ) {
                let _ = error_tx.send(CaptureEvent::Failed(e.to_string()));
            }
        },
        None,
    )
}
