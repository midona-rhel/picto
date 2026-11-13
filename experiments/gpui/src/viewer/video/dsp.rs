//! Sample arithmetic for the audio path: fitting decoded audio to the output
//! device's channels and rate, and changing speed without changing pitch, as
//! a media element's `playbackRate` does (`preservesPitch` is on by default).
//! All of it is streaming: chunks go in as they are decoded, and what can be
//! produced comes out.

/// Spreads `input` (interleaved, `from` channels) over `to` channels: mono
/// goes to both sides, the first two channels of anything wider are kept as
/// left and right, and extra output channels stay silent.
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

/// Linear-interpolating sample-rate conversion, carried across chunks.
pub struct Resampler {
    channels: usize,
    /// Input frames per output frame.
    step: f64,
    /// Where the next output frame falls, in input frames from the start of
    /// `previous` (the last frame of the chunk before).
    position: f64,
    previous: Option<Vec<f32>>,
}

impl Resampler {
    pub fn new(channels: usize, from: u32, to: u32) -> Self {
        Self {
            channels,
            step: from as f64 / to as f64,
            position: 0.,
            previous: None,
        }
    }

    pub fn passes_through(&self) -> bool {
        self.step == 1.
    }

    pub fn process(&mut self, input: &[f32]) -> Vec<f32> {
        if self.passes_through() || input.is_empty() {
            return input.to_vec();
        }
        let channels = self.channels;
        // The source is the previous chunk's last frame followed by this one.
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
        // Keep the last frame; positions count from it next time.
        self.previous = Some(input[input.len() - channels..].to_vec());
        self.position -= (frames - 1) as f64;
        output
    }
}

/// WSOLA time stretching: output is built from overlapping windows of the
/// input taken `rate` times faster than they are laid down, each nudged
/// within a small tolerance to where it best continues the previous one, so
/// speech and music keep their pitch.
pub struct Stretch {
    channels: usize,
    rate: f64,
    window: usize,
    hop: usize,
    tolerance: usize,
    hann: Vec<f32>,
    /// Interleaved input not yet needed no more, starting at absolute input
    /// frame `origin`.
    input: Vec<f32>,
    origin: usize,
    /// Segments laid down so far.
    segment: u64,
    /// Where the previous segment was taken from, in absolute input frames.
    previous: Option<usize>,
    /// The overlap-add accumulator, one window long.
    accumulator: Vec<f32>,
}

impl Stretch {
    /// `sample_rate` sets the window: 40 ms, half of it laid down per step,
    /// with a quarter window either way to search.
    pub fn new(channels: usize, sample_rate: u32, rate: f32) -> Self {
        let window = ((sample_rate as f64 * 0.04) as usize).max(64) & !1;
        let hop = window / 2;
        // A periodic Hann window: overlapped by half, its copies sum to one.
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

    pub fn passes_through(&self) -> bool {
        self.rate == 1.
    }

    fn frames(&self) -> usize {
        self.origin + self.input.len() / self.channels
    }

    /// The mono value of absolute input frame `frame`.
    fn mono(&self, frame: usize) -> f32 {
        let start = (frame - self.origin) * self.channels;
        self.input[start..start + self.channels].iter().sum::<f32>()
    }

    pub fn process(&mut self, input: &[f32]) -> Vec<f32> {
        if self.passes_through() {
            return input.to_vec();
        }
        self.input.extend_from_slice(input);
        let mut output = Vec::new();
        self.run(&mut output);
        output
    }

    /// Ends the stream: what is still held comes out, padded with silence.
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

    fn run(&mut self, output: &mut Vec<f32>) {
        let channels = self.channels;
        loop {
            let nominal = (self.segment as f64 * self.hop as f64 * self.rate).round() as usize;
            let low = nominal.saturating_sub(self.tolerance).max(self.origin);
            let high = nominal + self.tolerance;
            // The previous segment's natural continuation, to match.
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
            // Lay the windowed segment over the accumulator; its first half
            // is then complete.
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
            // Let go of input no later segment can reach.
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

    fn sine(frequency: f32, rate: u32, seconds: f32) -> Vec<f32> {
        (0..(rate as f32 * seconds) as usize)
            .flat_map(|i| {
                let value = (std::f32::consts::TAU * frequency * i as f32 / rate as f32).sin();
                [value, value]
            })
            .collect()
    }

    /// Upward zero crossings per second of the left channel.
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
            // Away from the padded ends the tone is still 440 Hz.
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
