//! Placeholder for platforms other than Windows: Media Foundation does not exist there.

use std::convert::Infallible;
use std::path::Path;
use std::time::Duration;

use crate::{AvailableEncoders, MediaError, Mp4Probe, Mp4Settings, Mp4Summary, ProbeOptions};

/// Never constructed on this platform: [`Mp4Writer::create`] always fails.
#[derive(Debug)]
#[cfg_attr(all(windows, test), expect(dead_code))]
pub struct Mp4Writer {
    never: Infallible,
}

#[cfg_attr(all(windows, test), expect(dead_code))]
impl Mp4Writer {
    pub fn create(_path: &Path, settings: &Mp4Settings) -> Result<Self, MediaError> {
        settings.validate()?;
        Err(MediaError::Unsupported)
    }

    pub fn video_encoder(&self) -> &crate::EncoderInfo {
        match self.never {}
    }

    pub fn push_frame(&mut self, _rgba: &[u8], _timestamp: Duration) -> Result<(), MediaError> {
        match self.never {}
    }

    pub fn push_audio(&mut self, _samples: &[i16]) -> Result<(), MediaError> {
        match self.never {}
    }

    pub fn finish(self) -> Result<Mp4Summary, MediaError> {
        match self.never {}
    }
}

pub fn available_encoders() -> Result<AvailableEncoders, MediaError> {
    Err(MediaError::Unsupported)
}

pub fn probe_mp4(_path: &Path, _options: ProbeOptions) -> Result<Mp4Probe, MediaError> {
    Err(MediaError::Unsupported)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn everything_is_unsupported() {
        let settings = Mp4Settings::new(1280, 720, 30);
        assert!(matches!(
            Mp4Writer::create(Path::new("x.mp4"), &settings),
            Err(MediaError::Unsupported)
        ));
        assert!(matches!(
            Mp4Writer::create(Path::new("x.mp4"), &Mp4Settings::new(3, 3, 30)),
            Err(MediaError::InvalidSettings(_))
        ));
        assert!(matches!(available_encoders(), Err(MediaError::Unsupported)));
        assert!(matches!(
            probe_mp4(Path::new("x.mp4"), ProbeOptions::default()),
            Err(MediaError::Unsupported)
        ));
    }
}
