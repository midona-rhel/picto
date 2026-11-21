//! The arithmetic on audio samples that playback needs: fitting decoded
//! audio to the number of channels and to the sample rate of the output
//! device, and changing the speed of audio without changing its pitch. Each
//! of the three works on a stream: it is given the chunks of samples as
//! they are decoded, and it returns the output that can be produced so far.

/// Returns `input`, which holds interleaved frames of `from` channels, as
/// interleaved frames of `to` channels. A mono input is copied to the first
/// two output channels. A mono output is the mean of the first two input
/// channels. In every other case the first two input channels become the
/// first two output channels and the other input channels are dropped.
/// Output channels after the second are silent.
pub fn map_channels(input: &[f32], from: usize, to: usize) -> Vec<f32> {
    if from == to || from == 0 {
        return input.to_vec();
    }
    let frames = input.len() / from;
    let mut output = vec![0.; frames * to];
    for frame in 0..frames {
        let source = &input[frame * from..(frame + 1) * from];
        let target = &mut output[frame * to..(frame + 1) * to];
        if from == 1 {
            for sample in target.iter_mut().take(2) {
                *sample = source[0];
            }
        } else if to == 1 {
            target[0] = (source[0] + source[1]) / 2.;
        } else {
            target[0] = source[0];
            target[1] = source[1];
        }
    }
    output
}

/// Converts audio from one sample rate to another by linear interpolation
/// between neighbouring frames. It keeps the last frame of each chunk, so
/// that it can interpolate between that frame and the first frame of the
/// next chunk.
pub struct Resampler {
    channels: usize,
    /// How many input frames one output frame advances by: the input
    /// sample rate divided by the output sample rate.
    step: f64,
    /// The position of the next output frame, in input frames. It is
    /// counted from `previous` when there is one, and from the first frame
    /// of the next chunk otherwise.
    position: f64,
    /// The last frame of the chunk before, or `None` before the first
    /// chunk.
    previous: Option<Vec<f32>>,
}

impl Resampler {
    /// Returns a resampler for interleaved audio of `channels` channels
    /// that converts from the sample rate `from` to the sample rate `to`.
    pub fn new(channels: usize, from: u32, to: u32) -> Self {
        Self {
            channels,
            step: from as f64 / to as f64,
            position: 0.,
            previous: None,
        }
    }

    /// Returns true when the two sample rates are equal. `process` then
    /// returns its input unchanged.
    pub fn passes_through(&self) -> bool {
        self.step == 1.
    }

    /// Converts the chunk `input` and returns the output frames that lie
    /// before the last frame of the chunk. That frame is kept for the next
    /// call.
    pub fn process(&mut self, input: &[f32]) -> Vec<f32> {
        if self.passes_through() || input.is_empty() {
            return input.to_vec();
        }
        let channels = self.channels;
        // The frames to interpolate between are the last frame of the chunk
        // before, when there is one, followed by the frames of this chunk.
        let offset = usize::from(self.previous.is_some());
        let frames = input.len() / channels + offset;
        let at = |frame: usize, channel: usize| -> f32 {
            match (&self.previous, frame) {
                (Some(previous), 0) => previous[channel],
                _ => input[(frame - offset) * channels + channel],
            }
        };
        let mut output = Vec::with_capacity(((frames as f64 / self.step) as usize + 1) * channels);
        while self.position + 1. < frames as f64 {
            let index = self.position.floor() as usize;
            let fraction = (self.position - index as f64) as f32;
            for channel in 0..channels {
                let a = at(index, channel);
                let b = at(index + 1, channel);
                output.push(a + (b - a) * fraction);
            }
            self.position += self.step;
        }
        // The last frame of this chunk is kept, and `position` is changed
        // so that it is counted from that frame.
        self.previous = Some(input[input.len() - channels..].to_vec());
        self.position -= (frames - 1) as f64;
        output
    }
}

/// Changes the speed of audio without changing its pitch, with the WSOLA
/// method (waveform similarity overlap-add). The output is made of segments
/// of the input, each one window long. Each segment is written to the
/// output half a window after the one before, so the segments overlap, and
/// they are added together. Each segment is taken from the input `rate`
/// times half a window after the one before. The place it is taken from is
/// moved by up to a quarter of a window, to where the input is most similar
/// to the input that followed the previous segment. The waveform then
/// continues from one segment into the next, so the pitch is kept.
pub struct Stretch {
    channels: usize,
    rate: f64,
    /// The length of a segment, in frames.
    window: usize,
    /// How far after the previous segment each segment is written to the
    /// output, in frames: half a window.
    hop: usize,
    /// The most that the place a segment is taken from may be moved in
    /// either direction, in frames: a quarter of a window.
    tolerance: usize,
    /// The weight of each frame of a segment.
    hann: Vec<f32>,
    /// The interleaved input that a later segment may still be taken from.
    /// Its first frame is frame `origin` of the whole input.
    input: Vec<f32>,
    /// The number of frames that have been dropped from the start of
    /// `input`.
    origin: usize,
    /// The number of segments written so far.
    segment: u64,
    /// The frame of the whole input at which the previous segment starts,
    /// or `None` before the first segment.
    previous: Option<usize>,
    /// One window of output that is not complete yet: the sum of the
    /// segments written so far that reach into it.
    accumulator: Vec<f32>,
}

impl Stretch {
    /// Returns a stretcher for interleaved audio of `channels` channels at
    /// the sample rate `sample_rate` that plays `rate` times as fast. The
    /// window is 40 ms long, at least 64 frames and an even number of
    /// frames. Each segment is written half a window after the one before,
    /// and the place it is taken from may be moved by up to a quarter of a
    /// window in either direction.
    pub fn new(channels: usize, sample_rate: u32, rate: f32) -> Self {
        let window = ((sample_rate as f64 * 0.04) as usize).max(64) & !1;
        let hop = window / 2;
        // The weights of a segment's frames are a periodic Hann window. Two
        // copies of it that overlap by half a window add up to 1 at every
        // frame, so the added segments are as loud as the input.
        let hann = (0..window)
            .map(|i| {
                let phase = std::f64::consts::TAU * i as f64 / window as f64;
                (0.5 - 0.5 * phase.cos()) as f32
            })
            .collect();
        Self {
            channels,
            rate: rate as f64,
            window,
            hop,
            tolerance: window / 4,
            hann,
            input: Vec::new(),
            origin: 0,
            segment: 0,
            previous: None,
            accumulator: vec![0.; window * channels],
        }
    }

    /// Returns true when the rate is 1. `process` then returns its input
    /// unchanged.
    pub fn passes_through(&self) -> bool {
        self.rate == 1.
    }

    /// Returns the number of input frames received so far, including those
    /// that have been dropped.
    fn frames(&self) -> usize {
        self.origin + self.input.len() / self.channels
    }

    /// Returns the sum of the channels of frame `frame` of the whole input.
    fn mono(&self, frame: usize) -> f32 {
        let start = (frame - self.origin) * self.channels;
        self.input[start..start + self.channels].iter().sum::<f32>()
    }

    /// Takes the chunk `input` and returns the output that can be produced
    /// so far.
    pub fn process(&mut self, input: &[f32]) -> Vec<f32> {
        if self.passes_through() {
            return input.to_vec();
        }
        self.input.extend_from_slice(input);
        let mut output = Vec::new();
        self.run(&mut output);
        output
    }

    /// Ends the stream and returns the rest of the output. Silence is added
    /// after the input, so that the segments that reach the end of the
    /// input can be written.
    pub fn finish(&mut self) -> Vec<f32> {
        if self.passes_through() {
            return Vec::new();
        }
        let padding = vec![0.; (self.window + 2 * self.tolerance) * self.channels];
        let mut output = Vec::new();
        self.input.extend_from_slice(&padding);
        self.run(&mut output);
        output.extend_from_slice(&self.accumulator[..self.hop * self.channels]);
        output
    }

    /// Writes segments to `output` for as long as enough input has arrived
    /// for the next segment.
    fn run(&mut self, output: &mut Vec<f32>) {
        let channels = self.channels;
        loop {
            let nominal = (self.segment as f64 * self.hop as f64 * self.rate).round() as usize;
            let low = nominal.saturating_sub(self.tolerance).max(self.origin);
            let high = nominal + self.tolerance;
            // `natural` is the input frame half a window after the start of
            // the previous segment. The input from there on is what followed
            // the previous segment in the input. Of the starts from `low` to
            // `high`, the new segment gets the one at which the input is
            // most similar to it, measured by the sum of the products of
            // the samples over half a window. Every second start and every
            // second sample are compared.
            let natural = self.previous.map(|previous| previous + self.hop);
            let needed = (high + self.window).max(natural.map_or(0, |n| n + self.hop));
            if self.frames() < needed {
                return;
            }
            let start = match natural {
                None => nominal.max(self.origin),
                Some(natural) => {
                    let mut best = (f32::MIN, low);
                    for candidate in (low..=high).step_by(2) {
                        let mut score = 0.;
                        for i in (0..self.hop).step_by(2) {
                            score += self.mono(natural + i) * self.mono(candidate + i);
                        }
                        if score > best.0 {
                            best = (score, candidate);
                        }
                    }
                    best.1
                }
            };
            // The segment, weighted by the Hann window, is added to the
            // accumulator. No later segment reaches into the first half of
            // the accumulator, so that half is written to the output. The
            // second half then moves to the front, and the space behind it
            // is cleared for the next segment.
            let base = (start - self.origin) * channels;
            for frame in 0..self.window {
                let weight = self.hann[frame];
                for channel in 0..channels {
                    self.accumulator[frame * channels + channel] +=
                        self.input[base + frame * channels + channel] * weight;
                }
            }
            output.extend_from_slice(&self.accumulator[..self.hop * channels]);
            self.accumulator.copy_within(self.hop * channels.., 0);
            let tail = self.accumulator.len() - self.hop * channels;
            self.accumulator[tail..].fill(0.);
            self.previous = Some(start);
            self.segment += 1;
            // The input that no later segment can be taken from or compared
            // with is dropped: everything before the earliest start of the
            // next segment, or before the next `natural` if that is earlier.
            let next = (self.segment as f64 * self.hop as f64 * self.rate).round() as usize;
            let keep = next.saturating_sub(self.tolerance).min(start + self.hop);
            if keep > self.origin {
                let drop = (keep - self.origin).min(self.input.len() / channels);
                self.input.drain(..drop * channels);
                self.origin += drop;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Resampler, Stretch, map_channels};

    /// Returns `seconds` of a stereo sine tone of `frequency` Hz at the
    /// sample rate `rate`, with the same samples in both channels.
    fn sine(frequency: f32, rate: u32, seconds: f32) -> Vec<f32> {
        (0..(rate as f32 * seconds) as usize)
            .flat_map(|i| {
                let value = (std::f32::consts::TAU * frequency * i as f32 / rate as f32).sin();
                [value, value]
            })
            .collect()
    }

    /// Returns the frequency of the left channel of the interleaved stereo
    /// `samples`, measured as the number of times a second that the signal
    /// goes from below zero to zero or above.
    fn frequency(samples: &[f32], rate: u32) -> f32 {
        let left: Vec<f32> = samples.iter().step_by(2).copied().collect();
        let crossings = left
            .windows(2)
            .filter(|pair| pair[0] < 0. && pair[1] >= 0.)
            .count();
        crossings as f32 / (left.len() as f32 / rate as f32)
    }

    #[test]
    fn channels_are_spread_and_folded() {
        assert_eq!(map_channels(&[1., 2.], 1, 2), vec![1., 1., 2., 2.]);
        assert_eq!(map_channels(&[1., 2., 3.], 3, 2), vec![1., 2.]);
        assert_eq!(map_channels(&[1., 3.], 2, 1), vec![2.]);
        assert_eq!(map_channels(&[1., 2.], 2, 4), vec![1., 2., 0., 0.]);
    }

    #[test]
    fn resampling_keeps_the_pitch_and_scales_the_length() {
        let input = sine(441., 44_100, 1.);
        let mut resampler = Resampler::new(2, 44_100, 48_000);
        let mut output = Vec::new();
        for chunk in input.chunks(1000 * 2) {
            output.extend(resampler.process(chunk));
        }
        let frames = output.len() / 2;
        assert!((frames as i64 - 48_000).abs() < 4, "{frames} frames");
        assert!((frequency(&output, 48_000) - 441.).abs() < 3.);
    }

    #[test]
    fn stretching_changes_the_length_and_keeps_the_pitch() {
        for rate in [0.5f32, 2., 3.] {
            let input = sine(440., 48_000, 2.);
            let mut stretch = Stretch::new(2, 48_000, rate);
            let mut output = Vec::new();
            for chunk in input.chunks(1024 * 2) {
                output.extend(stretch.process(chunk));
            }
            output.extend(stretch.finish());
            let seconds = output.len() as f32 / 2. / 48_000.;
            let expected = 2. / rate;
            assert!(
                (seconds - expected).abs() < 0.1,
                "rate {rate}: {seconds} s, expected {expected} s"
            );
            // The middle half of the output, which is away from the silence
            // that was added at the end, is still a tone of 440 Hz.
            let middle = &output[output.len() / 4 & !1..output.len() * 3 / 4 & !1];
            let measured = frequency(middle, 48_000);
            assert!((measured - 440.).abs() < 10., "rate {rate}: {measured} Hz");
        }
    }

    #[test]
    fn normal_speed_passes_samples_through() {
        let mut stretch = Stretch::new(2, 48_000, 1.);
        assert_eq!(stretch.process(&[0.5, -0.5]), vec![0.5, -0.5]);
        assert!(stretch.finish().is_empty());
    }
}
