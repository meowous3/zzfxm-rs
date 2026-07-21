//! A faithful Rust port of the [ZzFXM](https://github.com/keithclark/ZzFXM)
//! tracker-style music renderer.
//!
//! ZzFXM combines ZzFX instruments, reusable patterns, a sequence, and a BPM
//! into stereo PCM. This crate only renders samples; playback is left to the
//! application.

use std::collections::HashMap;
use std::error::Error;
use std::fmt;
use std::sync::Arc;
use zzfx_rs::{Zzfx, ZzfxParams};

pub const DEFAULT_BPM: f64 = 125.0;

#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Song {
    pub instruments: Vec<ZzfxParams>,
    pub patterns: Vec<Pattern>,
    pub sequence: Vec<usize>,
    pub bpm: f64,
    pub metadata: Metadata,
}

impl Song {
    pub fn new(
        instruments: Vec<ZzfxParams>,
        patterns: Vec<Pattern>,
        sequence: Vec<usize>,
        bpm: f64,
    ) -> Self {
        Self { instruments, patterns, sequence, bpm, metadata: Metadata::default() }
    }

    /// Convert the compact nested-array representation used by ZzFXM. Missing
    /// JavaScript array entries should be represented as `None`.
    pub fn from_compact(
        instruments: Vec<Vec<Option<f64>>>,
        patterns: Vec<Vec<Vec<Option<f64>>>>,
        sequence: Vec<usize>,
        bpm: Option<f64>,
    ) -> Result<Self, SongError> {
        let instruments = instruments
            .iter()
            .enumerate()
            .map(|(index, values)| instrument_from_compact(index, values))
            .collect::<Result<Vec<_>, _>>()?;
        let patterns = patterns
            .iter()
            .enumerate()
            .map(|(pattern_index, channels)| {
                let channels = channels
                    .iter()
                    .enumerate()
                    .map(|(channel_index, values)| {
                        channel_from_compact(pattern_index, channel_index, values)
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(Pattern { channels })
            })
            .collect::<Result<Vec<_>, SongError>>()?;
        let song = Self::new(instruments, patterns, sequence, bpm.unwrap_or(DEFAULT_BPM));
        song.validate()?;
        Ok(song)
    }

    pub fn validate(&self) -> Result<(), SongError> {
        if !self.bpm.is_finite() || self.bpm <= 0.0 {
            return Err(SongError::InvalidBpm(self.bpm));
        }
        for (sequence_index, &pattern_index) in self.sequence.iter().enumerate() {
            let pattern = self.patterns.get(pattern_index).ok_or(SongError::PatternOutOfRange {
                sequence_index,
                pattern_index,
            })?;
            let first = pattern.channels.first().ok_or(SongError::PatternHasNoChannels(pattern_index))?;
            if first.notes.is_empty() {
                return Err(SongError::FirstChannelHasNoNotes(pattern_index));
            }
        }
        for (pattern_index, pattern) in self.patterns.iter().enumerate() {
            for (channel_index, channel) in pattern.channels.iter().enumerate() {
                if !channel.pan.is_finite() {
                    return Err(SongError::InvalidPan { pattern_index, channel_index });
                }
                for &note in &channel.notes {
                    if !note.is_finite() || note < i32::MIN as f64 || note > i32::MAX as f64 {
                        return Err(SongError::InvalidNote { pattern_index, channel_index, note });
                    }
                    if note.trunc() as i32 != 0 && channel.instrument >= self.instruments.len() {
                        return Err(SongError::InstrumentOutOfRange {
                            pattern_index,
                            channel_index,
                            instrument: channel.instrument,
                        });
                    }
                }
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Metadata {
    pub title: Option<String>,
    pub author: Option<String>,
    pub notes: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Pattern {
    pub channels: Vec<Channel>,
}

impl Pattern {
    pub fn new(channels: Vec<Channel>) -> Self {
        Self { channels }
    }
}

#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Channel {
    pub instrument: usize,
    pub pan: f64,
    /// Integer portions are notes, fractional portions are attenuation, zero is
    /// a rest, and a negative note releases the current instrument.
    pub notes: Vec<f64>,
}

impl Channel {
    pub fn new(instrument: usize, pan: f64, notes: Vec<f64>) -> Self {
        Self { instrument, pan, notes }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct StereoSamples {
    pub left: Vec<f32>,
    pub right: Vec<f32>,
    pub sample_rate: u32,
}

impl StereoSamples {
    pub fn len(&self) -> usize {
        self.left.len()
    }

    pub fn is_empty(&self) -> bool {
        self.left.is_empty()
    }

    pub fn duration_seconds(&self) -> f64 {
        self.len() as f64 / self.sample_rate as f64
    }

    pub fn interleaved(&self) -> impl Iterator<Item = f32> + '_ {
        self.left.iter().zip(&self.right).flat_map(|(&left, &right)| [left, right])
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Renderer {
    pub zzfx: Zzfx,
}

impl Default for Renderer {
    fn default() -> Self {
        Self { zzfx: Zzfx::default() }
    }
}

impl Renderer {
    pub const fn new(zzfx: Zzfx) -> Self {
        Self { zzfx }
    }

    pub fn render(&self, song: &Song) -> Result<StereoSamples, SongError> {
        self.render_with(song, |params| self.zzfx.build_samples_legacy(params))
    }

    pub fn render_with_random(
        &self,
        song: &Song,
        mut random: impl FnMut() -> f64,
    ) -> Result<StereoSamples, SongError> {
        self.render_with(song, |params| {
            self.zzfx.build_samples_legacy_with_random(params, &mut random)
        })
    }

    fn render_with(
        &self,
        song: &Song,
        mut synthesize: impl FnMut(ZzfxParams) -> Vec<f32>,
    ) -> Result<StereoSamples, SongError> {
        song.validate()?;
        if song.sequence.is_empty() {
            return Ok(StereoSamples {
                left: Vec::new(),
                right: Vec::new(),
                sample_rate: self.zzfx.sample_rate,
            });
        }

        let beat_length = ((self.zzfx.sample_rate as f64 / song.bpm * 60.0).trunc() as usize) >> 2;
        let channel_count = song
            .sequence
            .iter()
            .map(|&index| song.patterns[index].channels.len())
            .max()
            .unwrap_or(0);
        let mut cache: HashMap<(usize, i32), Arc<[f32]>> = HashMap::new();
        let mut left = Vec::<f64>::new();
        let mut right = Vec::<f64>::new();

        for channel_index in 0..channel_count {
            let mut sample_buffer: Arc<[f32]> = Arc::from([]);
            let mut sample_offset = 0usize;
            let mut output_offset = 0usize;
            let mut not_first_beat = false;
            let mut current_instrument = None;
            let mut attenuation = 0.0f64;
            let mut panning = 0.0f64;

            for (sequence_index, &pattern_index) in song.sequence.iter().enumerate() {
                let pattern = &song.patterns[pattern_index];
                let fallback = Channel { instrument: 0, pan: 0.0, notes: vec![0.0] };
                let channel = pattern.channels.get(channel_index).unwrap_or(&fallback);
                let first_note_count = pattern.channels[0].notes.len();
                let duration_beats = first_note_count - usize::from(!not_first_beat);
                let next_output_offset = output_offset + duration_beats * beat_length;
                let sequence_end = sequence_index + 1 == song.sequence.len();
                let iterations = channel.notes.len() + usize::from(sequence_end);
                let mut output_index = output_offset;

                for beat_index in 0..iterations {
                    let note = channel.notes.get(beat_index).copied().unwrap_or(0.0);
                    let note_integer = note.trunc() as i32;
                    let stop = (sequence_end && beat_index + 1 == iterations)
                        || current_instrument != Some(channel.instrument)
                        || note_integer != 0;

                    if not_first_beat {
                        for sample_index in 0..beat_length {
                            let source = sample_buffer.get(sample_offset).copied().unwrap_or(0.0) as f64;
                            sample_offset += 1;
                            let sample = (1.0 - attenuation) * source / 2.0;
                            if output_index == left.len() {
                                left.push(0.0);
                                right.push(0.0);
                            } else if output_index > left.len() {
                                left.resize(output_index + 1, 0.0);
                                right.resize(output_index + 1, 0.0);
                            }
                            left[output_index] += sample * (1.0 - panning);
                            right[output_index] += sample * (1.0 + panning);
                            output_index += 1;

                            if sample_index as i64 > beat_length as i64 - 99 && stop && attenuation < 1.0 {
                                attenuation += 1.0 / 99.0;
                            }
                        }
                    }

                    if note != 0.0 {
                        attenuation = note % 1.0;
                        panning = channel.pan;
                        if note_integer != 0 {
                            current_instrument = Some(channel.instrument);
                            sample_offset = 0;
                            if note_integer > 0 {
                                let key = (channel.instrument, note_integer);
                                if !cache.contains_key(&key) {
                                    let mut params = song.instruments[channel.instrument];
                                    params.frequency *= 2.0f64.powf((note_integer as f64 - 12.0) / 12.0);
                                    cache.insert(key, synthesize(params).into());
                                }
                                sample_buffer = Arc::clone(&cache[&key]);
                            } else {
                                sample_buffer = Arc::from([]);
                            }
                        }
                    }
                    not_first_beat = true;
                }
                output_offset = next_output_offset;
            }
        }

        let length = left.len().max(right.len());
        left.resize(length, 0.0);
        right.resize(length, 0.0);
        Ok(StereoSamples {
            left: left.into_iter().map(|sample| sample as f32).collect(),
            right: right.into_iter().map(|sample| sample as f32).collect(),
            sample_rate: self.zzfx.sample_rate,
        })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum SongError {
    InvalidBpm(f64),
    PatternOutOfRange { sequence_index: usize, pattern_index: usize },
    PatternHasNoChannels(usize),
    FirstChannelHasNoNotes(usize),
    InstrumentOutOfRange { pattern_index: usize, channel_index: usize, instrument: usize },
    InvalidPan { pattern_index: usize, channel_index: usize },
    InvalidNote { pattern_index: usize, channel_index: usize, note: f64 },
    InvalidCompactInstrument { instrument: usize, parameter_count: usize },
    InvalidCompactChannel { pattern: usize, channel: usize },
}

impl fmt::Display for SongError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidBpm(bpm) => write!(formatter, "invalid BPM: {bpm}"),
            Self::PatternOutOfRange { sequence_index, pattern_index } => write!(
                formatter,
                "sequence entry {sequence_index} references missing pattern {pattern_index}"
            ),
            Self::PatternHasNoChannels(pattern) => write!(formatter, "pattern {pattern} has no channels"),
            Self::FirstChannelHasNoNotes(pattern) => {
                write!(formatter, "pattern {pattern}'s first channel has no notes")
            }
            Self::InstrumentOutOfRange { pattern_index, channel_index, instrument } => write!(
                formatter,
                "pattern {pattern_index} channel {channel_index} references missing instrument {instrument}"
            ),
            Self::InvalidPan { pattern_index, channel_index } => {
                write!(formatter, "pattern {pattern_index} channel {channel_index} has an invalid pan")
            }
            Self::InvalidNote { pattern_index, channel_index, note } => write!(
                formatter,
                "pattern {pattern_index} channel {channel_index} has invalid note {note}"
            ),
            Self::InvalidCompactInstrument { instrument, parameter_count } => write!(
                formatter,
                "compact instrument {instrument} has {parameter_count} parameters; expected at most 21"
            ),
            Self::InvalidCompactChannel { pattern, channel } => write!(
                formatter,
                "compact pattern {pattern} channel {channel} needs instrument and pan entries"
            ),
        }
    }
}

impl Error for SongError {}

fn instrument_from_compact(index: usize, values: &[Option<f64>]) -> Result<ZzfxParams, SongError> {
    if values.len() > 21 {
        return Err(SongError::InvalidCompactInstrument {
            instrument: index,
            parameter_count: values.len(),
        });
    }
    let mut params = ZzfxParams::default();
    macro_rules! set {
        ($position:expr, $field:ident) => {
            if let Some(Some(value)) = values.get($position) {
                params.$field = *value;
            }
        };
    }
    set!(0, volume);
    set!(1, randomness);
    set!(2, frequency);
    set!(3, attack);
    set!(4, sustain);
    set!(5, release);
    if let Some(Some(value)) = values.get(6) {
        params.shape = value.trunc().clamp(0.0, u8::MAX as f64) as u8;
    }
    set!(7, shape_curve);
    set!(8, slide);
    set!(9, delta_slide);
    set!(10, pitch_jump);
    set!(11, pitch_jump_time);
    set!(12, repeat_time);
    set!(13, noise);
    set!(14, modulation);
    set!(15, bit_crush);
    set!(16, delay);
    set!(17, sustain_volume);
    set!(18, decay);
    set!(19, tremolo);
    set!(20, filter);
    Ok(params)
}

fn channel_from_compact(
    pattern: usize,
    channel: usize,
    values: &[Option<f64>],
) -> Result<Channel, SongError> {
    if values.len() < 2 {
        return Err(SongError::InvalidCompactChannel { pattern, channel });
    }
    let instrument_value = values[0].unwrap_or(0.0);
    if !instrument_value.is_finite()
        || instrument_value < 0.0
        || instrument_value.fract() != 0.0
        || instrument_value > usize::MAX as f64
    {
        return Err(SongError::InvalidCompactChannel { pattern, channel });
    }
    Ok(Channel {
        instrument: instrument_value as usize,
        pan: values[1].unwrap_or(0.0),
        notes: values[2..].iter().map(|note| note.unwrap_or(0.0)).collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reference_song() -> Song {
        let instrument = ZzfxParams::new(0.5, 440.0, 0.01, 1.0, 0.2);
        Song::new(
            vec![instrument],
            vec![Pattern::new(vec![Channel::new(0, -0.5, vec![12.0, 0.0, 14.5, 0.0])])],
            vec![0],
            120.0,
        )
    }

    #[test]
    fn matches_zzfxm_2_0_3_reference_mix() {
        let rendered = Renderer::default()
            .render_with_random(&reference_song(), || 0.5)
            .unwrap();
        assert_eq!(rendered.len(), 22_048);
        for (index, left, right) in [
            (1, 0.000_013_051_735_f32, 0.000_004_350_578_f32),
            (100, -0.000_296_814_66_f32, -0.000_098_938_22_f32),
            (1_000, -0.015_974_361_f32, -0.005_324_787_f32),
            (5_511, -0.010_563_249_f32, -0.003_521_083_f32),
            (16_536, -0.055_798_132_f32, -0.018_599_378_f32),
            (22_047, -0.000_180_909_99_f32, -0.000_060_303_333_f32),
        ] {
            assert!((rendered.left[index] - left).abs() < 2e-7, "left sample {index}");
            assert!((rendered.right[index] - right).abs() < 2e-7, "right sample {index}");
        }
    }

    #[test]
    fn compact_song_uses_javascript_defaults_for_holes() {
        let song = Song::from_compact(
            vec![vec![Some(0.5), None, Some(440.0)]],
            vec![vec![vec![None, Some(1.0), Some(12.5), None]]],
            vec![0],
            None,
        )
        .unwrap();
        assert_eq!(song.bpm, 125.0);
        assert_eq!(song.instruments[0].randomness, 0.05);
        assert_eq!(song.patterns[0].channels[0].instrument, 0);
        assert_eq!(song.patterns[0].channels[0].notes, vec![12.5, 0.0]);
    }

    #[test]
    fn rejects_invalid_references_without_panicking() {
        let mut song = reference_song();
        song.sequence = vec![7];
        assert!(matches!(
            Renderer::default().render(&song),
            Err(SongError::PatternOutOfRange { pattern_index: 7, .. })
        ));
    }

    #[test]
    fn empty_sequence_renders_empty_stereo() {
        let mut song = reference_song();
        song.sequence.clear();
        let rendered = Renderer::default().render(&song).unwrap();
        assert!(rendered.is_empty());
        assert_eq!(rendered.sample_rate, zzfx_rs::DEFAULT_SAMPLE_RATE);
    }
}
