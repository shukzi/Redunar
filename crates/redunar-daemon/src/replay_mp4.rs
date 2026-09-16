use crate::{
    EncodedReplayPacket, ReplayAudioSnapshot, ReplayRing, ReplayVideoCodec, ReplayVideoStream,
};
use redunar_capture_audio::EncodedOpusPacket;
use std::error::Error;
use std::fmt;
use std::io::{self, Write};

const TIMESCALE: u32 = 1_000_000;
const TRACK_ID: u32 = 1;
const AUDIO_TRACK_ID: u32 = 2;
const MAX_FRAGMENT_DURATION_NS: u64 = 30_000_000_000;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Mp4VideoSummary {
    pub packets: u32,
    pub fragments: u32,
    pub duration_ns: u64,
    pub bytes_written: u64,
}

#[derive(Debug)]
pub enum Mp4VideoError {
    EmptyRing,
    MissingInitialKeyframe,
    UnsupportedCodec,
    InvalidPacket,
    TimestampOverflow,
    ContainerTooLarge,
    Io(io::Error),
}

impl fmt::Display for Mp4VideoError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyRing => formatter.write_str("replay ring contains no encoded video"),
            Self::MissingInitialKeyframe => {
                formatter.write_str("replay clip does not start at a keyframe")
            }
            Self::UnsupportedCodec => formatter.write_str("MP4 Replay currently requires H.264"),
            Self::InvalidPacket => formatter.write_str("replay packet cannot be muxed"),
            Self::TimestampOverflow => formatter.write_str("replay clip timestamp overflows"),
            Self::ContainerTooLarge => formatter.write_str("replay container exceeds MP4 limits"),
            Self::Io(error) => write!(formatter, "could not write replay container: {error}"),
        }
    }
}

impl Error for Mp4VideoError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            _ => None,
        }
    }
}

impl From<io::Error> for Mp4VideoError {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}

/// Write one seek-free fragmented MP4 from a bounded H.264 packet ring.
///
/// # Errors
///
/// Returns an error for unsupported codecs, invalid packet framing, timestamp
/// or box-size overflow, or an output write failure.
pub fn write_video_only_mp4(
    destination: &mut impl Write,
    stream: &ReplayVideoStream,
    ring: &ReplayRing,
) -> Result<Mp4VideoSummary, Mp4VideoError> {
    if stream.codec() != ReplayVideoCodec::H264 {
        return Err(Mp4VideoError::UnsupportedCodec);
    }
    let packets = ring.packets().collect::<Vec<_>>();
    let first = packets.first().ok_or(Mp4VideoError::EmptyRing)?;
    if !first.is_keyframe() {
        return Err(Mp4VideoError::MissingInitialKeyframe);
    }
    for packet in &packets {
        stream
            .validate_packet(packet.bytes())
            .map_err(|_| Mp4VideoError::InvalidPacket)?;
    }

    let mut output = CountingWriter::new(destination);
    output.write_all(&ftyp()?)?;
    output.write_all(&moov(stream)?)?;
    let origin = first.timestamp_ns();
    let mut fragments = 0_u32;
    let mut start = 0_usize;
    while start < packets.len() {
        let mut end = start + 1;
        while end < packets.len() {
            let elapsed = packets[end]
                .timestamp_ns()
                .saturating_sub(packets[start].timestamp_ns());
            if packets[end].is_keyframe() || elapsed >= MAX_FRAGMENT_DURATION_NS {
                break;
            }
            end += 1;
        }
        write_fragment(
            &mut output,
            fragments.saturating_add(1),
            origin,
            &packets[start..end],
        )?;
        fragments = fragments.saturating_add(1);
        start = end;
    }
    let last = packets.last().ok_or(Mp4VideoError::EmptyRing)?;
    let duration_ns = last
        .timestamp_ns()
        .checked_add(last.duration_ns())
        .and_then(|end| end.checked_sub(origin))
        .ok_or(Mp4VideoError::TimestampOverflow)?;
    Ok(Mp4VideoSummary {
        packets: u32::try_from(packets.len()).unwrap_or(u32::MAX),
        fragments,
        duration_ns,
        bytes_written: output.written,
    })
}

/// Write fragmented MP4 with H.264 video and synchronized Opus game audio.
///
/// # Errors
///
/// Returns [`Mp4VideoError`] for invalid video/audio packets, timestamps,
/// unsupported stream metadata, box overflow, or output failure.
pub fn write_audio_video_mp4(
    destination: &mut impl Write,
    stream: &ReplayVideoStream,
    ring: &ReplayRing,
    audio: &ReplayAudioSnapshot,
) -> Result<Mp4VideoSummary, Mp4VideoError> {
    if stream.codec() != ReplayVideoCodec::H264 {
        return Err(Mp4VideoError::UnsupportedCodec);
    }
    let packets = ring.packets().collect::<Vec<_>>();
    let first = packets.first().ok_or(Mp4VideoError::EmptyRing)?;
    if !first.is_keyframe() {
        return Err(Mp4VideoError::MissingInitialKeyframe);
    }
    for packet in &packets {
        stream
            .validate_packet(packet.bytes())
            .map_err(|_| Mp4VideoError::InvalidPacket)?;
    }
    let origin = first.timestamp_ns();
    let video_end = packets
        .last()
        .and_then(|packet| packet.timestamp_ns().checked_add(packet.duration_ns()))
        .ok_or(Mp4VideoError::TimestampOverflow)?;
    let audio_packets = audio
        .packets
        .iter()
        .filter(|packet| packet.timestamp_ns >= origin && packet.timestamp_ns < video_end)
        .collect::<Vec<_>>();
    if audio_packets
        .iter()
        .any(|packet| packet.bytes.is_empty() || packet.bytes.len() > 4_096)
    {
        return Err(Mp4VideoError::InvalidPacket);
    }
    let mut output = CountingWriter::new(destination);
    output.write_all(&ftyp()?)?;
    output.write_all(&moov_with_audio(stream, audio)?)?;
    let mut fragments = 0_u32;
    let mut start = 0_usize;
    while start < packets.len() {
        let mut end = start + 1;
        while end < packets.len() {
            let elapsed = packets[end]
                .timestamp_ns()
                .saturating_sub(packets[start].timestamp_ns());
            if packets[end].is_keyframe() || elapsed >= MAX_FRAGMENT_DURATION_NS {
                break;
            }
            end += 1;
        }
        fragments = fragments.saturating_add(1);
        write_fragment(&mut output, fragments, origin, &packets[start..end])?;
        start = end;
    }
    for chunk in audio_packets.chunks(1_500) {
        fragments = fragments.saturating_add(1);
        write_audio_fragment(&mut output, fragments, origin, chunk)?;
    }
    Ok(Mp4VideoSummary {
        packets: u32::try_from(packets.len()).unwrap_or(u32::MAX),
        fragments,
        duration_ns: video_end.saturating_sub(origin),
        bytes_written: output.written,
    })
}

fn moov_with_audio(
    stream: &ReplayVideoStream,
    audio: &ReplayAudioSnapshot,
) -> Result<Vec<u8>, Mp4VideoError> {
    let mut contents = Vec::new();
    push_box(&mut contents, *b"mvhd", &mvhd_for_next_track(3))?;
    push_box(&mut contents, *b"trak", &trak(stream)?)?;
    push_box(&mut contents, *b"trak", &audio_trak(audio)?)?;
    let mut mvex = Vec::new();
    push_box(&mut mvex, *b"trex", &trex())?;
    push_box(&mut mvex, *b"trex", &trex_for(AUDIO_TRACK_ID))?;
    push_box(&mut contents, *b"mvex", &mvex)?;
    make_box(*b"moov", &contents)
}

fn audio_trak(audio: &ReplayAudioSnapshot) -> Result<Vec<u8>, Mp4VideoError> {
    let mut contents = Vec::new();
    let mut tkhd = full_box(0, 7);
    tkhd.extend_from_slice(&[0; 8]);
    put_u32(&mut tkhd, AUDIO_TRACK_ID);
    tkhd.extend_from_slice(&[0; 12]);
    put_u16(&mut tkhd, 0);
    put_u16(&mut tkhd, 0);
    put_u16(&mut tkhd, 0x0100);
    put_u16(&mut tkhd, 0);
    matrix(&mut tkhd);
    tkhd.extend_from_slice(&[0; 8]);
    push_box(&mut contents, *b"tkhd", &tkhd)?;
    let mut mdia = Vec::new();
    let mut mdhd = full_box(0, 0);
    mdhd.extend_from_slice(&[0; 8]);
    put_u32(&mut mdhd, audio.stream.sample_rate);
    put_u32(&mut mdhd, 0);
    put_u16(&mut mdhd, 0x55c4);
    put_u16(&mut mdhd, 0);
    push_box(&mut mdia, *b"mdhd", &mdhd)?;
    let mut hdlr = full_box(0, 0);
    put_u32(&mut hdlr, 0);
    hdlr.extend_from_slice(b"soun");
    hdlr.extend_from_slice(&[0; 12]);
    hdlr.extend_from_slice(b"Redunar Audio\0");
    push_box(&mut mdia, *b"hdlr", &hdlr)?;
    push_box(&mut mdia, *b"minf", &audio_minf(audio)?)?;
    push_box(&mut contents, *b"mdia", &mdia)?;
    Ok(contents)
}

fn audio_minf(audio: &ReplayAudioSnapshot) -> Result<Vec<u8>, Mp4VideoError> {
    let mut value = Vec::new();
    let mut smhd = full_box(0, 0);
    smhd.extend_from_slice(&[0; 4]);
    push_box(&mut value, *b"smhd", &smhd)?;
    let mut dref = full_box(0, 0);
    put_u32(&mut dref, 1);
    push_box(&mut dref, *b"url ", &full_box(0, 1))?;
    let mut dinf = Vec::new();
    push_box(&mut dinf, *b"dref", &dref)?;
    push_box(&mut value, *b"dinf", &dinf)?;
    let mut stbl = Vec::new();
    let mut stsd = full_box(0, 0);
    put_u32(&mut stsd, 1);
    push_box(&mut stsd, *b"Opus", &opus_sample_entry(audio)?)?;
    push_box(&mut stbl, *b"stsd", &stsd)?;
    for kind in [*b"stts", *b"stsc"] {
        let mut empty = full_box(0, 0);
        put_u32(&mut empty, 0);
        push_box(&mut stbl, kind, &empty)?;
    }
    let mut sample_sizes = full_box(0, 0);
    put_u32(&mut sample_sizes, 0);
    put_u32(&mut sample_sizes, 0);
    push_box(&mut stbl, *b"stsz", &sample_sizes)?;
    let mut stco = full_box(0, 0);
    put_u32(&mut stco, 0);
    push_box(&mut stbl, *b"stco", &stco)?;
    push_box(&mut value, *b"stbl", &stbl)?;
    Ok(value)
}

fn opus_sample_entry(audio: &ReplayAudioSnapshot) -> Result<Vec<u8>, Mp4VideoError> {
    let mut value = vec![0; 6];
    put_u16(&mut value, 1);
    value.extend_from_slice(&[0; 8]);
    put_u16(&mut value, u16::from(audio.stream.channels));
    put_u16(&mut value, 16);
    put_u16(&mut value, 0);
    put_u16(&mut value, 0);
    put_u32(&mut value, audio.stream.sample_rate << 16);
    let mut dops = Vec::new();
    dops.push(0);
    dops.push(audio.stream.channels);
    put_u16(&mut dops, 312);
    put_u32(&mut dops, audio.stream.sample_rate);
    put_u16(&mut dops, 0);
    dops.push(0);
    push_box(&mut value, *b"dOps", &dops)?;
    Ok(value)
}

fn trex_for(track_id: u32) -> Vec<u8> {
    let mut value = full_box(0, 0);
    put_u32(&mut value, track_id);
    put_u32(&mut value, 1);
    value.extend_from_slice(&[0; 12]);
    value
}

fn write_audio_fragment(
    output: &mut impl Write,
    sequence: u32,
    origin_ns: u64,
    packets: &[&EncodedOpusPacket],
) -> Result<(), Mp4VideoError> {
    let base_time = packets[0]
        .timestamp_ns
        .checked_sub(origin_ns)
        .and_then(|value| value.checked_mul(48_000))
        .map(|value| value / 1_000_000_000)
        .ok_or(Mp4VideoError::TimestampOverflow)?;
    let provisional = audio_moof(sequence, base_time, packets, 0)?;
    let offset = i32::try_from(provisional.len().saturating_add(8))
        .map_err(|_| Mp4VideoError::ContainerTooLarge)?;
    let moof = audio_moof(sequence, base_time, packets, offset)?;
    output.write_all(&moof)?;
    let payload = packets
        .iter()
        .try_fold(0_u64, |total, packet| {
            total.checked_add(u64::try_from(packet.bytes.len()).unwrap_or(u64::MAX))
        })
        .ok_or(Mp4VideoError::ContainerTooLarge)?;
    write_box_header(output, *b"mdat", payload)?;
    for packet in packets {
        output.write_all(&packet.bytes)?;
    }
    Ok(())
}

fn audio_moof(
    sequence: u32,
    base_time: u64,
    packets: &[&EncodedOpusPacket],
    data_offset: i32,
) -> Result<Vec<u8>, Mp4VideoError> {
    let mut mfhd = full_box(0, 0);
    put_u32(&mut mfhd, sequence);
    let mut tfhd = full_box(0, 0x0002_0000);
    put_u32(&mut tfhd, AUDIO_TRACK_ID);
    let mut tfdt = full_box(1, 0);
    put_u64(&mut tfdt, base_time);
    let mut trun = full_box(0, 0x0000_0301);
    put_u32(
        &mut trun,
        u32::try_from(packets.len()).map_err(|_| Mp4VideoError::ContainerTooLarge)?,
    );
    trun.extend_from_slice(&data_offset.to_be_bytes());
    for packet in packets {
        put_u32(&mut trun, 960);
        put_u32(
            &mut trun,
            u32::try_from(packet.bytes.len()).map_err(|_| Mp4VideoError::ContainerTooLarge)?,
        );
    }
    let mut traf = Vec::new();
    push_box(&mut traf, *b"tfhd", &tfhd)?;
    push_box(&mut traf, *b"tfdt", &tfdt)?;
    push_box(&mut traf, *b"trun", &trun)?;
    let mut contents = Vec::new();
    push_box(&mut contents, *b"mfhd", &mfhd)?;
    push_box(&mut contents, *b"traf", &traf)?;
    make_box(*b"moof", &contents)
}

fn ftyp() -> Result<Vec<u8>, Mp4VideoError> {
    make_box(*b"ftyp", b"isom\0\0\x02\0isomiso6avc1mp41")
}

fn moov(stream: &ReplayVideoStream) -> Result<Vec<u8>, Mp4VideoError> {
    let mut contents = Vec::new();
    push_box(&mut contents, *b"mvhd", &mvhd())?;
    push_box(&mut contents, *b"trak", &trak(stream)?)?;
    let mut mvex = Vec::new();
    push_box(&mut mvex, *b"trex", &trex())?;
    push_box(&mut contents, *b"mvex", &mvex)?;
    make_box(*b"moov", &contents)
}

fn mvhd() -> Vec<u8> {
    mvhd_for_next_track(TRACK_ID + 1)
}

fn mvhd_for_next_track(next_track_id: u32) -> Vec<u8> {
    let mut value = full_box(0, 0);
    put_u32(&mut value, 0);
    put_u32(&mut value, 0);
    put_u32(&mut value, TIMESCALE);
    put_u32(&mut value, 0);
    put_u32(&mut value, 0x0001_0000);
    put_u16(&mut value, 0x0100);
    value.extend_from_slice(&[0; 10]);
    matrix(&mut value);
    value.extend_from_slice(&[0; 24]);
    put_u32(&mut value, next_track_id);
    value
}

fn trak(stream: &ReplayVideoStream) -> Result<Vec<u8>, Mp4VideoError> {
    let mut contents = Vec::new();
    push_box(&mut contents, *b"tkhd", &tkhd(stream))?;
    push_box(&mut contents, *b"mdia", &mdia(stream)?)?;
    Ok(contents)
}

fn tkhd(stream: &ReplayVideoStream) -> Vec<u8> {
    let mut value = full_box(0, 7);
    put_u32(&mut value, 0);
    put_u32(&mut value, 0);
    put_u32(&mut value, TRACK_ID);
    put_u32(&mut value, 0);
    put_u32(&mut value, 0);
    value.extend_from_slice(&[0; 8]);
    put_u16(&mut value, 0);
    put_u16(&mut value, 0);
    put_u16(&mut value, 0);
    put_u16(&mut value, 0);
    matrix(&mut value);
    put_u32(&mut value, stream.width() << 16);
    put_u32(&mut value, stream.height() << 16);
    value
}

fn mdia(stream: &ReplayVideoStream) -> Result<Vec<u8>, Mp4VideoError> {
    let mut value = Vec::new();
    push_box(&mut value, *b"mdhd", &mdhd())?;
    push_box(&mut value, *b"hdlr", &hdlr())?;
    push_box(&mut value, *b"minf", &minf(stream)?)?;
    Ok(value)
}

fn mdhd() -> Vec<u8> {
    let mut value = full_box(0, 0);
    put_u32(&mut value, 0);
    put_u32(&mut value, 0);
    put_u32(&mut value, TIMESCALE);
    put_u32(&mut value, 0);
    put_u16(&mut value, 0x55c4);
    put_u16(&mut value, 0);
    value
}

fn hdlr() -> Vec<u8> {
    let mut value = full_box(0, 0);
    put_u32(&mut value, 0);
    value.extend_from_slice(b"vide");
    value.extend_from_slice(&[0; 12]);
    value.extend_from_slice(b"Redunar Video\0");
    value
}

fn minf(stream: &ReplayVideoStream) -> Result<Vec<u8>, Mp4VideoError> {
    let mut value = Vec::new();
    let mut vmhd = full_box(0, 1);
    vmhd.extend_from_slice(&[0; 8]);
    push_box(&mut value, *b"vmhd", &vmhd)?;
    let mut dref = full_box(0, 0);
    put_u32(&mut dref, 1);
    push_box(&mut dref, *b"url ", &full_box(0, 1))?;
    let mut dinf = Vec::new();
    push_box(&mut dinf, *b"dref", &dref)?;
    push_box(&mut value, *b"dinf", &dinf)?;
    push_box(&mut value, *b"stbl", &stbl(stream)?)?;
    Ok(value)
}

fn stbl(stream: &ReplayVideoStream) -> Result<Vec<u8>, Mp4VideoError> {
    let mut value = Vec::new();
    let mut stsd = full_box(0, 0);
    put_u32(&mut stsd, 1);
    push_box(&mut stsd, *b"avc1", &avc1(stream)?)?;
    push_box(&mut value, *b"stsd", &stsd)?;
    for kind in [*b"stts", *b"stsc"] {
        let mut empty = full_box(0, 0);
        put_u32(&mut empty, 0);
        push_box(&mut value, kind, &empty)?;
    }
    let mut sample_sizes = full_box(0, 0);
    put_u32(&mut sample_sizes, 0);
    put_u32(&mut sample_sizes, 0);
    push_box(&mut value, *b"stsz", &sample_sizes)?;
    let mut stco = full_box(0, 0);
    put_u32(&mut stco, 0);
    push_box(&mut value, *b"stco", &stco)?;
    Ok(value)
}

fn avc1(stream: &ReplayVideoStream) -> Result<Vec<u8>, Mp4VideoError> {
    let mut value = vec![0; 6];
    put_u16(&mut value, 1);
    value.extend_from_slice(&[0; 16]);
    put_u16(
        &mut value,
        u16::try_from(stream.width()).map_err(|_| Mp4VideoError::ContainerTooLarge)?,
    );
    put_u16(
        &mut value,
        u16::try_from(stream.height()).map_err(|_| Mp4VideoError::ContainerTooLarge)?,
    );
    put_u32(&mut value, 0x0048_0000);
    put_u32(&mut value, 0x0048_0000);
    put_u32(&mut value, 0);
    put_u16(&mut value, 1);
    value.extend_from_slice(&[0; 32]);
    put_u16(&mut value, 0x0018);
    put_u16(&mut value, 0xffff);
    push_box(&mut value, *b"avcC", stream.codec_private())?;
    Ok(value)
}

fn trex() -> Vec<u8> {
    let mut value = full_box(0, 0);
    put_u32(&mut value, TRACK_ID);
    put_u32(&mut value, 1);
    put_u32(&mut value, 0);
    put_u32(&mut value, 0);
    put_u32(&mut value, 0);
    value
}

fn write_fragment(
    output: &mut impl Write,
    sequence: u32,
    origin_ns: u64,
    packets: &[&EncodedReplayPacket],
) -> Result<(), Mp4VideoError> {
    let base_time = to_timescale(
        packets[0]
            .timestamp_ns()
            .checked_sub(origin_ns)
            .ok_or(Mp4VideoError::TimestampOverflow)?,
    )?;
    let provisional = moof(sequence, base_time, packets, 0)?;
    let data_offset = i32::try_from(provisional.len().saturating_add(8))
        .map_err(|_| Mp4VideoError::ContainerTooLarge)?;
    let moof = moof(sequence, base_time, packets, data_offset)?;
    output.write_all(&moof)?;
    let payload_bytes = packets
        .iter()
        .try_fold(0_u64, |total, packet| {
            total.checked_add(u64::try_from(packet.bytes().len()).unwrap_or(u64::MAX))
        })
        .ok_or(Mp4VideoError::ContainerTooLarge)?;
    write_box_header(output, *b"mdat", payload_bytes)?;
    for packet in packets {
        output.write_all(packet.bytes())?;
    }
    Ok(())
}

fn moof(
    sequence: u32,
    base_time: u64,
    packets: &[&EncodedReplayPacket],
    data_offset: i32,
) -> Result<Vec<u8>, Mp4VideoError> {
    let mut mfhd = full_box(0, 0);
    put_u32(&mut mfhd, sequence);
    let mut tfhd = full_box(0, 0x0002_0000);
    put_u32(&mut tfhd, TRACK_ID);
    let mut tfdt = full_box(1, 0);
    put_u64(&mut tfdt, base_time);
    let mut trun = full_box(0, 0x0000_0701);
    put_u32(
        &mut trun,
        u32::try_from(packets.len()).map_err(|_| Mp4VideoError::ContainerTooLarge)?,
    );
    trun.extend_from_slice(&data_offset.to_be_bytes());
    for (index, packet) in packets.iter().enumerate() {
        let duration_ns = packets.get(index + 1).map_or(packet.duration_ns(), |next| {
            next.timestamp_ns().saturating_sub(packet.timestamp_ns())
        });
        put_u32(
            &mut trun,
            u32::try_from(to_timescale(duration_ns)?)
                .map_err(|_| Mp4VideoError::TimestampOverflow)?,
        );
        put_u32(
            &mut trun,
            u32::try_from(packet.bytes().len()).map_err(|_| Mp4VideoError::ContainerTooLarge)?,
        );
        put_u32(
            &mut trun,
            if packet.is_keyframe() {
                0x0200_0000
            } else {
                0x0101_0000
            },
        );
    }
    let mut traf = Vec::new();
    push_box(&mut traf, *b"tfhd", &tfhd)?;
    push_box(&mut traf, *b"tfdt", &tfdt)?;
    push_box(&mut traf, *b"trun", &trun)?;
    let mut contents = Vec::new();
    push_box(&mut contents, *b"mfhd", &mfhd)?;
    push_box(&mut contents, *b"traf", &traf)?;
    make_box(*b"moof", &contents)
}

fn to_timescale(nanoseconds: u64) -> Result<u64, Mp4VideoError> {
    nanoseconds
        .checked_mul(u64::from(TIMESCALE))
        .map(|value| value / 1_000_000_000)
        .ok_or(Mp4VideoError::TimestampOverflow)
}

fn matrix(value: &mut Vec<u8>) {
    for item in [0x0001_0000, 0, 0, 0, 0x0001_0000, 0, 0, 0, 0x4000_0000] {
        put_u32(value, item);
    }
}

fn full_box(version: u8, flags: u32) -> Vec<u8> {
    let bytes = flags.to_be_bytes();
    vec![version, bytes[1], bytes[2], bytes[3]]
}

fn make_box(kind: [u8; 4], contents: &[u8]) -> Result<Vec<u8>, Mp4VideoError> {
    let mut value = Vec::with_capacity(contents.len().saturating_add(8));
    push_box(&mut value, kind, contents)?;
    Ok(value)
}

fn push_box(
    destination: &mut Vec<u8>,
    kind: [u8; 4],
    contents: &[u8],
) -> Result<(), Mp4VideoError> {
    let size = u32::try_from(contents.len().saturating_add(8))
        .map_err(|_| Mp4VideoError::ContainerTooLarge)?;
    put_u32(destination, size);
    destination.extend_from_slice(&kind);
    destination.extend_from_slice(contents);
    Ok(())
}

fn write_box_header(
    output: &mut impl Write,
    kind: [u8; 4],
    payload: u64,
) -> Result<(), Mp4VideoError> {
    let size = payload
        .checked_add(8)
        .ok_or(Mp4VideoError::ContainerTooLarge)?;
    let size = u32::try_from(size).map_err(|_| Mp4VideoError::ContainerTooLarge)?;
    output.write_all(&size.to_be_bytes())?;
    output.write_all(&kind)?;
    Ok(())
}

fn put_u16(value: &mut Vec<u8>, number: u16) {
    value.extend_from_slice(&number.to_be_bytes());
}
fn put_u32(value: &mut Vec<u8>, number: u32) {
    value.extend_from_slice(&number.to_be_bytes());
}
fn put_u64(value: &mut Vec<u8>, number: u64) {
    value.extend_from_slice(&number.to_be_bytes());
}

struct CountingWriter<'a, W> {
    inner: &'a mut W,
    written: u64,
}
impl<'a, W> CountingWriter<'a, W> {
    const fn new(inner: &'a mut W) -> Self {
        Self { inner, written: 0 }
    }
}
impl<W: Write> Write for CountingWriter<'_, W> {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        let count = self.inner.write(buffer)?;
        self.written = self
            .written
            .saturating_add(u64::try_from(count).unwrap_or(u64::MAX));
        Ok(count)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ReplayPacketFormat, ReplayVideoCodec};
    use redunar_core::ReplaySettings;
    use std::fs;
    use std::process::Command;

    fn stream() -> ReplayVideoStream {
        ReplayVideoStream::new(
            ReplayVideoCodec::H264,
            ReplayPacketFormat::H264LengthPrefixed4,
            1_920,
            1_080,
            60,
            vec![
                1, 66, 0, 30, 0xff, 0xe1, 0, 2, 0x67, 0x42, 1, 0, 2, 0x68, 0xce,
            ],
        )
        .expect("valid stream")
    }

    #[test]
    fn fragmented_mp4_contains_initialization_and_media_boxes() {
        let mut ring = ReplayRing::new(ReplaySettings::default());
        for (timestamp, keyframe, nal) in [
            (1_u64, true, 0x65),
            (16_666_668, false, 0x41),
            (2_000_000_001, true, 0x65),
        ] {
            ring.push(
                EncodedReplayPacket::new(
                    timestamp,
                    16_666_667,
                    keyframe,
                    vec![0, 0, 0, 2, nal, 0x88],
                )
                .expect("packet"),
            )
            .expect("ring");
        }
        let mut output = Vec::new();
        let summary = write_video_only_mp4(&mut output, &stream(), &ring).expect("MP4");
        assert_eq!(&output[4..8], b"ftyp");
        for kind in [b"moov", b"avcC", b"moof", b"mdat"] {
            assert!(output.windows(4).any(|window| window == kind));
        }
        assert_eq!(summary.packets, 3);
        assert_eq!(summary.fragments, 2);
        assert_eq!(summary.bytes_written, output.len() as u64);
    }

    #[test]
    fn audio_video_mp4_contains_opus_track_and_audio_fragment() {
        let mut ring = ReplayRing::new(ReplaySettings::default());
        ring.push(
            EncodedReplayPacket::new(1, 16_666_667, true, vec![0, 0, 0, 2, 0x65, 0x88])
                .expect("packet"),
        )
        .expect("ring");
        let audio = ReplayAudioSnapshot {
            stream: redunar_capture_audio::OpusStreamDescription::default(),
            packets: vec![EncodedOpusPacket {
                timestamp_ns: 1,
                duration_ns: 20_000_000,
                bytes: vec![0xf8, 0xff, 0xfe],
            }],
        };
        let mut output = Vec::new();
        let summary =
            write_audio_video_mp4(&mut output, &stream(), &ring, &audio).expect("audio/video MP4");
        for kind in [b"Opus", b"dOps", b"soun"] {
            assert!(output.windows(4).any(|window| window == kind));
        }
        assert_eq!(summary.fragments, 2);
    }

    #[test]
    fn mp4_rejects_empty_or_non_keyframe_history() {
        let empty = ReplayRing::new(ReplaySettings::default());
        assert!(matches!(
            write_video_only_mp4(&mut Vec::new(), &stream(), &empty),
            Err(Mp4VideoError::EmptyRing)
        ));
    }

    #[test]
    #[ignore = "requires the optional ffprobe executable"]
    fn fragmented_mp4_is_recognized_by_ffprobe() {
        let mut ring = ReplayRing::new(ReplaySettings::default());
        ring.push(
            EncodedReplayPacket::new(1, 16_666_667, true, vec![0, 0, 0, 2, 0x65, 0x88])
                .expect("packet"),
        )
        .expect("ring");
        let mut output = Vec::new();
        write_video_only_mp4(&mut output, &stream(), &ring).expect("MP4");
        let path =
            std::env::temp_dir().join(format!("redunar-mp4-ffprobe-{}.mp4", std::process::id()));
        fs::write(&path, output).expect("write MP4 fixture");
        let probe = Command::new("ffprobe")
            .args([
                "-v",
                "error",
                "-show_entries",
                "stream=codec_name",
                "-of",
                "default=nw=1",
            ])
            .arg(&path)
            .output()
            .expect("run ffprobe");
        let _ = fs::remove_file(path);
        assert!(
            probe.status.success(),
            "{}",
            String::from_utf8_lossy(&probe.stderr)
        );
        assert!(String::from_utf8_lossy(&probe.stdout).contains("codec_name=h264"));
    }

    #[test]
    #[ignore = "requires the optional ffprobe executable and local libopus"]
    fn audio_video_mp4_reports_h264_and_opus_streams() {
        let mut ring = ReplayRing::new(ReplaySettings::default());
        ring.push(
            EncodedReplayPacket::new(1, 16_666_667, true, vec![0, 0, 0, 2, 0x65, 0x88])
                .expect("packet"),
        )
        .expect("ring");
        let mut encoder = redunar_capture_audio::OpusEncoder::open().expect("libopus");
        let samples = vec![
            0_i16;
            redunar_capture_audio::AUDIO_FRAME_SAMPLES_PER_CHANNEL
                * usize::from(redunar_capture_audio::AUDIO_CHANNELS)
        ];
        let audio = ReplayAudioSnapshot {
            stream: encoder.stream(),
            packets: vec![encoder.encode_frame(1, &samples).expect("Opus")],
        };
        let mut output = Vec::new();
        write_audio_video_mp4(&mut output, &stream(), &ring, &audio).expect("MP4");
        let path =
            std::env::temp_dir().join(format!("redunar-av-mp4-ffprobe-{}.mp4", std::process::id()));
        fs::write(&path, output).expect("write MP4");
        let probe = Command::new("ffprobe")
            .args([
                "-v",
                "error",
                "-show_entries",
                "stream=codec_name,codec_type",
                "-of",
                "default=nw=1",
            ])
            .arg(&path)
            .output()
            .expect("ffprobe");
        let _ = fs::remove_file(path);
        assert!(
            probe.status.success(),
            "{}",
            String::from_utf8_lossy(&probe.stderr)
        );
        let stdout = String::from_utf8_lossy(&probe.stdout);
        assert!(stdout.contains("codec_name=h264"), "{stdout}");
        assert!(stdout.contains("codec_name=opus"), "{stdout}");
    }
}
