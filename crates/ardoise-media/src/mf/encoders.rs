//! Lists the H.264 and AAC encoders registered with Media Foundation.

use windows::Win32::Media::MediaFoundation::{
    IMFActivate, MFAudioFormat_AAC, MFMediaType_Audio, MFMediaType_Video,
    MFT_CATEGORY_AUDIO_ENCODER, MFT_CATEGORY_VIDEO_ENCODER, MFT_ENUM_FLAG_ALL,
    MFT_ENUM_FLAG_SORTANDFILTER, MFT_ENUM_HARDWARE_URL_Attribute, MFT_FRIENDLY_NAME_Attribute,
    MFT_REGISTER_TYPE_INFO, MFT_TRANSFORM_CLSID_Attribute, MFTEnumEx, MFVideoFormat_H264,
};
use windows::Win32::System::Com::CoTaskMemFree;
use windows::core::GUID;

use super::{MfSession, StepExt, get_string};
use crate::{AvailableEncoders, EncoderInfo, MediaError};

/// The H.264 and AAC encoders Media Foundation offers on this PC, in the order it prefers
/// them (hardware first). Empty lists mean MP4 export through Media Foundation is impossible.
pub fn available_encoders() -> Result<AvailableEncoders, MediaError> {
    let _session = MfSession::start()?;
    let infos = |list: Vec<Registered>| list.into_iter().map(|r| r.info).collect();
    Ok(AvailableEncoders {
        h264: infos(h264_encoders()?),
        aac: infos(list(
            MFT_CATEGORY_AUDIO_ENCODER,
            MFMediaType_Audio,
            MFAudioFormat_AAC,
        )?),
    })
}

/// A registered encoder and its class identifier.
pub(crate) struct Registered {
    pub(crate) clsid: Option<GUID>,
    pub(crate) info: EncoderInfo,
}

/// The H.264 encoders, best first. Needs an open [`MfSession`].
pub(crate) fn h264_encoders() -> Result<Vec<Registered>, MediaError> {
    list(
        MFT_CATEGORY_VIDEO_ENCODER,
        MFMediaType_Video,
        MFVideoFormat_H264,
    )
}

fn list(category: GUID, major: GUID, subtype: GUID) -> Result<Vec<Registered>, MediaError> {
    let output = MFT_REGISTER_TYPE_INFO {
        guidMajorType: major,
        guidSubtype: subtype,
    };
    let mut array: *mut Option<IMFActivate> = std::ptr::null_mut();
    let mut count = 0u32;
    // SAFETY: `output` and the two out-pointers are valid locals. On success Media Foundation
    // allocates `count` activation objects with CoTaskMemAlloc; they are released below.
    unsafe {
        MFTEnumEx(
            category,
            MFT_ENUM_FLAG_ALL | MFT_ENUM_FLAG_SORTANDFILTER,
            None,
            Some(&output),
            &mut array,
            &mut count,
        )
    }
    .step("MFTEnumEx")?;
    if array.is_null() {
        return Ok(Vec::new());
    }

    // SAFETY: MFTEnumEx returned `count` initialised entries at `array`, which we own.
    let activates = unsafe { std::slice::from_raw_parts_mut(array, count as usize) };
    let encoders = activates
        .iter_mut()
        .filter_map(Option::take) // takes ownership: each reference is released when dropped
        .map(|activate| Registered {
            // SAFETY: valid activation object; constant key.
            clsid: unsafe { activate.GetGUID(&MFT_TRANSFORM_CLSID_Attribute) }.ok(),
            info: EncoderInfo {
                name: get_string(&activate, &MFT_FRIENDLY_NAME_Attribute),
                hardware: get_string(&activate, &MFT_ENUM_HARDWARE_URL_Attribute).is_some(),
            },
        })
        .collect();
    // SAFETY: the array came from CoTaskMemAlloc and every entry was taken out above.
    unsafe { CoTaskMemFree(Some(array.cast_const().cast())) };
    Ok(encoders)
}
