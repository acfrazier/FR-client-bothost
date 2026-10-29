//! Bounded geometry shared by panel and client GPU readback.

use super::readback::ReadbackError;

/// Maximum raw capture image size selected by the T4.4 product envelope.
pub const RECORD_MAX_RAW_PIXELS: usize = 4_000_000;
/// Maximum row-padded GPU staging allocation selected by the product envelope.
pub const READBACK_MAX_STAGE_BYTES: usize = 16_000_000;

/// Dimensions of one texture readback. Validation is performed before the
/// staging buffer is allocated, by [`super::readback::ReadbackBudget`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CaptureGeometry {
    width: u32,
    height: u32,
}

impl CaptureGeometry {
    pub const fn new(width: u32, height: u32) -> Self {
        Self { width, height }
    }

    pub const fn width(self) -> u32 {
        self.width
    }

    pub const fn height(self) -> u32 {
        self.height
    }

    pub(crate) fn checked_layout(self) -> Result<(usize, usize, u32), ReadbackError> {
        let width = usize::try_from(self.width).map_err(|_| ReadbackError::Limit)?;
        let height = usize::try_from(self.height).map_err(|_| ReadbackError::Limit)?;
        let pixels = width.checked_mul(height).ok_or(ReadbackError::Limit)?;
        if pixels == 0 || pixels > RECORD_MAX_RAW_PIXELS {
            return Err(ReadbackError::Limit);
        }
        let raw_bytes = pixels.checked_mul(4).ok_or(ReadbackError::Limit)?;
        if raw_bytes > READBACK_MAX_STAGE_BYTES {
            return Err(ReadbackError::Limit);
        }
        let raw_row = width.checked_mul(4).ok_or(ReadbackError::Limit)?;
        let alignment = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT as usize;
        let padded_row = raw_row
            .checked_add(alignment - 1)
            .ok_or(ReadbackError::Limit)?
            / alignment
            * alignment;
        let stage_bytes = padded_row.checked_mul(height).ok_or(ReadbackError::Limit)?;
        if stage_bytes > READBACK_MAX_STAGE_BYTES {
            return Err(ReadbackError::Limit);
        }
        let padded_row = u32::try_from(padded_row).map_err(|_| ReadbackError::Limit)?;
        Ok((raw_bytes, stage_bytes, padded_row))
    }
}
