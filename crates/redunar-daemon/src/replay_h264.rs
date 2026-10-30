use crate::{EncodedReplayPacket, ReplayEncoderError};

const MAX_PARAMETER_SET_BYTES: usize = u16::MAX as usize;
const H264_NAL_TYPE_MASK: u8 = 0x1f;
const H264_NAL_TYPE_IDR: u8 = 5;
const H264_NAL_TYPE_SPS: u8 = 7;
const H264_NAL_TYPE_PPS: u8 = 8;

/// Validated H.264 sequence and picture parameter sets returned by a hardware
/// encoder session. Redunar retains exactly one SPS/PPS pair for one coded
/// stream epoch; a resize creates a new epoch and new codec private data.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct H264ParameterSets {
    sequence: Box<[u8]>,
    picture: Box<[u8]>,
}

impl H264ParameterSets {
    /// Extract one SPS and one PPS from an Annex B parameter byte stream.
    ///
    /// # Errors
    ///
    /// Returns [`ReplayEncoderError`] for malformed byte-stream framing,
    /// missing parameter sets, duplicates, or parameter sets larger than the
    /// AVC decoder-configuration record can represent.
    pub fn from_annex_b(bytes: &[u8]) -> Result<Self, ReplayEncoderError> {
        let units = annex_b_units(bytes)?;
        let mut sequence = None;
        let mut picture = None;
        for unit in units {
            match nal_type(unit)? {
                H264_NAL_TYPE_SPS if sequence.is_none() => sequence = Some(unit),
                H264_NAL_TYPE_PPS if picture.is_none() => picture = Some(unit),
                H264_NAL_TYPE_SPS | H264_NAL_TYPE_PPS => {
                    return Err(ReplayEncoderError::InvalidCodecHeaders);
                }
                _ => {}
            }
        }
        let (Some(sequence), Some(picture)) = (sequence, picture) else {
            return Err(ReplayEncoderError::InvalidCodecHeaders);
        };
        if sequence.len() < 4
            || sequence.len() > MAX_PARAMETER_SET_BYTES
            || picture.len() > MAX_PARAMETER_SET_BYTES
        {
            return Err(ReplayEncoderError::InvalidCodecHeaders);
        }
        Ok(Self {
            sequence: sequence.into(),
            picture: picture.into(),
        })
    }

    /// Build ISO/IEC 14496-15 `AVCDecoderConfigurationRecord` data for the
    /// Matroska `CodecPrivate` element. NAL lengths use four bytes throughout.
    ///
    /// # Panics
    ///
    /// This cannot panic for an instance created through [`Self::from_annex_b`],
    /// which validates SPS length and profile bytes before construction.
    #[must_use]
    pub fn avc_decoder_configuration(&self) -> Vec<u8> {
        let sequence_length =
            u16::try_from(self.sequence.len()).expect("validated H.264 SPS length must fit in u16");
        let picture_length =
            u16::try_from(self.picture.len()).expect("validated H.264 PPS length must fit in u16");
        let mut result = Vec::with_capacity(11 + self.sequence.len() + self.picture.len());
        result.extend_from_slice(&[
            1,
            self.sequence[1],
            self.sequence[2],
            self.sequence[3],
            0xff,
            0xe1,
        ]);
        result.extend_from_slice(&sequence_length.to_be_bytes());
        result.extend_from_slice(&self.sequence);
        result.push(1);
        result.extend_from_slice(&picture_length.to_be_bytes());
        result.extend_from_slice(&self.picture);
        result
    }

    #[must_use]
    pub fn sequence(&self) -> &[u8] {
        &self.sequence
    }

    #[must_use]
    pub fn picture(&self) -> &[u8] {
        &self.picture
    }
}

/// Convert one hardware-produced H.264 Annex B access unit into the
/// four-byte length-prefixed representation used by Redunar's packet ring.
/// Keyframe state is derived from the NAL units rather than trusted from a
/// backend flag.
///
/// # Errors
///
/// Returns [`ReplayEncoderError`] for malformed Annex B framing, oversized
/// NAL units, an access unit without a VCL NAL, or an invalid packet shape.
pub fn h264_annex_b_access_unit(
    timestamp_ns: u64,
    duration_ns: u64,
    bytes: &[u8],
) -> Result<EncodedReplayPacket, ReplayEncoderError> {
    let units = annex_b_units(bytes)?;
    let mut payload = Vec::with_capacity(bytes.len());
    let mut keyframe = false;
    let mut has_vcl = false;
    for unit in units {
        let unit_type = nal_type(unit)?;
        if (1..=5).contains(&unit_type) {
            has_vcl = true;
        }
        keyframe |= unit_type == H264_NAL_TYPE_IDR;
        let length =
            u32::try_from(unit.len()).map_err(|_| ReplayEncoderError::InvalidPacketFraming)?;
        payload.extend_from_slice(&length.to_be_bytes());
        payload.extend_from_slice(unit);
    }
    if !has_vcl {
        return Err(ReplayEncoderError::InvalidPacketFraming);
    }
    EncodedReplayPacket::new(timestamp_ns, duration_ns, keyframe, payload)
        .map_err(ReplayEncoderError::Packet)
}

fn annex_b_units(bytes: &[u8]) -> Result<Vec<&[u8]>, ReplayEncoderError> {
    let Some((mut unit_start, _)) = start_code_at(bytes, 0) else {
        return Err(ReplayEncoderError::InvalidPacketFraming);
    };
    let mut units = Vec::new();
    loop {
        let next = find_start_code(bytes, unit_start);
        let unit_end = next.map_or(bytes.len(), |(position, _)| position);
        let unit = bytes
            .get(unit_start..unit_end)
            .filter(|unit| !unit.is_empty())
            .ok_or(ReplayEncoderError::InvalidPacketFraming)?;
        units.push(unit);
        let Some((position, length)) = next else {
            break;
        };
        unit_start = position
            .checked_add(length)
            .ok_or(ReplayEncoderError::InvalidPacketFraming)?;
        if unit_start >= bytes.len() {
            return Err(ReplayEncoderError::InvalidPacketFraming);
        }
    }
    Ok(units)
}

fn start_code_at(bytes: &[u8], position: usize) -> Option<(usize, usize)> {
    let tail = bytes.get(position..)?;
    if tail.starts_with(&[0, 0, 0, 1]) {
        Some((position + 4, 4))
    } else if tail.starts_with(&[0, 0, 1]) {
        Some((position + 3, 3))
    } else {
        None
    }
}

fn find_start_code(bytes: &[u8], from: usize) -> Option<(usize, usize)> {
    (from..bytes.len())
        .find_map(|position| start_code_at(bytes, position).map(|(_, length)| (position, length)))
}

fn nal_type(unit: &[u8]) -> Result<u8, ReplayEncoderError> {
    let header = *unit
        .first()
        .ok_or(ReplayEncoderError::InvalidPacketFraming)?;
    if header & 0x80 != 0 {
        return Err(ReplayEncoderError::InvalidPacketFraming);
    }
    Ok(header & H264_NAL_TYPE_MASK)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SPS: &[u8] = &[0x67, 0x64, 0x00, 0x28, 0xac, 0xd9, 0x40];
    const PPS: &[u8] = &[0x68, 0xee, 0x3c, 0x80];

    fn annex_b(units: &[&[u8]]) -> Vec<u8> {
        let mut bytes = Vec::new();
        for (index, unit) in units.iter().enumerate() {
            bytes.extend_from_slice(if index % 2 == 0 {
                &[0, 0, 0, 1]
            } else {
                &[0, 0, 1]
            });
            bytes.extend_from_slice(unit);
        }
        bytes
    }

    #[test]
    fn parameter_sets_build_valid_avc_decoder_configuration() {
        let sets = H264ParameterSets::from_annex_b(&annex_b(&[SPS, PPS])).expect("headers");
        let configuration = sets.avc_decoder_configuration();
        assert_eq!(&configuration[..6], &[1, 0x64, 0, 0x28, 0xff, 0xe1]);
        assert_eq!(u16::from_be_bytes([configuration[6], configuration[7]]), 7);
        assert_eq!(sets.sequence(), SPS);
        assert_eq!(sets.picture(), PPS);
    }

    #[test]
    fn access_unit_conversion_preserves_nals_and_detects_idr() {
        let sei = [0x06, 0x05, 0xff];
        let idr = [0x65, 0x88, 0x84];
        let packet =
            h264_annex_b_access_unit(10, 16_666_667, &annex_b(&[&sei, &idr])).expect("packet");
        assert!(packet.is_keyframe());
        assert_eq!(packet.timestamp_ns(), 10);
        assert_eq!(
            packet.bytes(),
            &[0, 0, 0, 3, 0x06, 0x05, 0xff, 0, 0, 0, 3, 0x65, 0x88, 0x84]
        );
    }

    #[test]
    fn access_unit_without_vcl_is_rejected() {
        assert_eq!(
            h264_annex_b_access_unit(1, 16_666_667, &annex_b(&[SPS, PPS])),
            Err(ReplayEncoderError::InvalidPacketFraming)
        );
    }

    #[test]
    fn malformed_or_duplicate_headers_are_rejected() {
        assert_eq!(
            H264ParameterSets::from_annex_b(&[0, 0, 1]),
            Err(ReplayEncoderError::InvalidPacketFraming)
        );
        assert_eq!(
            H264ParameterSets::from_annex_b(&annex_b(&[SPS, SPS, PPS])),
            Err(ReplayEncoderError::InvalidCodecHeaders)
        );
    }
}
