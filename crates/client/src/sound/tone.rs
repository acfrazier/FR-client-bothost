//! Port of `~/experiments/Server/webclient/src/sound/{Envelope,Filter,Tone}.ts`.
//! The TS statics that are never mutated after first fill (`Tone.sine`,
//! `Tone.noise`) become process-wide `OnceLock`s; the per-`Tone` filter
//! coefficient statics become fields, since only one tone generates at a
//! time. The shared `Tone.buf` scratch is passed in by the caller so it can
//! live on the `Client`-owned `JagFX` table (design: mutable statics on the
//! client). `Int32Array` out-of-range reads/writes are silently dropped in
//! JS; the `scratch_get`/`scratch_set` helpers keep that behaviour.

use std::sync::OnceLock;

use crate::io::Packet;

/// `Tone.sine` from TS: `(Math.sin(i / 5215.1903) * 16384.0) | 0`.
fn sine_table() -> &'static [i32; 32768] {
    static SINE: OnceLock<[i32; 32768]> = OnceLock::new();
    SINE.get_or_init(|| {
        let mut table = [0i32; 32768];
        for (i, slot) in table.iter_mut().enumerate() {
            *slot = ((i as f64 / 5215.1903).sin() * 16384.0) as i32;
        }
        table
    })
}

/// `Tone.noise` from TS: a fixed ±1 table (seeded so it is reproducible).
fn noise_table() -> &'static [i32; 32768] {
    static NOISE: OnceLock<[i32; 32768]> = OnceLock::new();
    NOISE.get_or_init(|| {
        let mut state: u64 = 0x274_b0d;
        let mut table = [0i32; 32768];
        for slot in table.iter_mut() {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            *slot = if state & 1 == 1 { 1 } else { -1 };
        }
        table
    })
}

#[inline]
fn scratch_get(buf: &[i32], index: usize) -> i32 {
    buf.get(index).copied().unwrap_or(0)
}

#[inline]
fn scratch_add(buf: &mut [i32], index: usize, value: i32) {
    if let Some(slot) = buf.get_mut(index) {
        *slot = slot.wrapping_add(value);
    }
}

#[inline]
fn scratch_set(buf: &mut [i32], index: usize, value: i32) {
    if let Some(slot) = buf.get_mut(index) {
        *slot = value;
    }
}

impl Default for Envelope {
    fn default() -> Self {
        Self::new()
    }
}

/// `Envelope.ts`: a 2D shape played by `genNext`. `amplitude`/`delta` stay
/// f64 like the JS Numbers; the `| 0` truncations happen where TS has them.
#[derive(Clone)]
pub struct Envelope {
    length: i32,
    shape_delta: Vec<i32>,
    shape_peak: Vec<i32>,
    pub start: i32,
    pub end: i32,
    form: i32,
    threshold: i32,
    position: i32,
    delta: f64,
    amplitude: f64,
    ticks: i32,
}

impl Envelope {
    pub fn new() -> Self {
        Envelope {
            length: 0,
            shape_delta: Vec::new(),
            shape_peak: Vec::new(),
            start: 0,
            end: 0,
            form: 0,
            threshold: 0,
            position: 0,
            delta: 0.0,
            amplitude: 0.0,
            ticks: 0,
        }
    }

    pub fn load(buf: &mut Packet) -> Self {
        let mut envelope = Self::new();
        envelope.form = buf.g1();
        envelope.start = buf.g4();
        envelope.end = buf.g4();
        envelope.load_points(buf);
        envelope
    }

    /// `loadPoints(buf)` from TS; also called by `Filter.unpack`.
    pub fn load_points(&mut self, buf: &mut Packet) {
        self.length = buf.g1();
        self.shape_delta = vec![0; self.length as usize];
        self.shape_peak = vec![0; self.length as usize];
        for i in 0..self.length as usize {
            self.shape_delta[i] = buf.g2();
            self.shape_peak[i] = buf.g2();
        }
    }

    pub fn gen_init(&mut self) {
        self.threshold = 0;
        self.position = 0;
        self.delta = 0.0;
        self.amplitude = 0.0;
        self.ticks = 0;
    }

    pub fn gen_next(&mut self, delta: i32) -> i32 {
        if self.length == 0 {
            return 0;
        }
        if self.ticks >= self.threshold {
            self.amplitude = (self.shape_peak[self.position as usize] << 15) as f64;
            self.position += 1;
            if self.position >= self.length {
                self.position = self.length - 1;
            }
            self.threshold = ((self.shape_delta[self.position as usize] as f64 / 65536.0) * delta as f64) as i32;
            if self.threshold > self.ticks {
                self.delta = (((self.shape_peak[self.position as usize] << 15) as f64 - self.amplitude)
                    / (self.threshold - self.ticks) as f64) as i32 as f64;
            }
        }
        self.amplitude += self.delta;
        self.ticks += 1;
        ((self.amplitude - self.delta) as i32) >> 15
    }
}

/// `Filter.ts`: an IIR filter whose coefficients are recomputed every 128
/// samples as the `filterRange` envelope plays.
#[derive(Clone)]
pub struct Filter {
    pairs: [i32; 2],
    frequencies: [[[i32; 4]; 2]; 2],
    ranges: [[[i32; 4]; 2]; 2],
    unities: [i32; 2],
    coeff: [[f64; 8]; 2],
    coeff_int: [[i32; 8]; 2],
    reduce_coeff: f64,
    reduce_coeff_int: i32,
}

impl Default for Filter {
    fn default() -> Self {
        Filter {
            pairs: [0; 2],
            frequencies: [[[0; 4]; 2]; 2],
            ranges: [[[0; 4]; 2]; 2],
            unities: [0; 2],
            coeff: [[0.0; 8]; 2],
            coeff_int: [[0; 8]; 2],
            reduce_coeff: 0.0,
            reduce_coeff_int: 0,
        }
    }
}

impl Filter {
    /// `unpack(buf, envelope)` from TS.
    pub fn unpack(&mut self, buf: &mut Packet, envelope: &mut Envelope) {
        let count = buf.g1();
        self.pairs[0] = count >> 4;
        self.pairs[1] = count & 0xf;

        if count != 0 {
            self.unities[0] = buf.g2();
            self.unities[1] = buf.g2();

            let migration = buf.g1();

            for direction in 0..2 {
                for pair in 0..self.pairs[direction] as usize {
                    self.frequencies[direction][0][pair] = buf.g2();
                    self.ranges[direction][0][pair] = buf.g2();
                }
            }

            for direction in 0..2 {
                for pair in 0..self.pairs[direction] as usize {
                    if migration & (1 << (direction * 4 + pair)) != 0 {
                        self.frequencies[direction][1][pair] = buf.g2();
                        self.ranges[direction][1][pair] = buf.g2();
                    } else {
                        self.frequencies[direction][1][pair] = self.frequencies[direction][0][pair];
                        self.ranges[direction][1][pair] = self.ranges[direction][0][pair];
                    }
                }
            }

            if migration != 0 || self.unities[1] != self.unities[0] {
                envelope.load_points(buf);
            }
        } else {
            self.unities[0] = 0;
            self.unities[1] = 0;
        }
    }

    fn radius(&self, pair: usize, direction: usize, t: f64) -> f64 {
        let base = self.ranges[direction][0][pair] as f64;
        let range = self.ranges[direction][1][pair] as f64;
        let value = base + t * (range - base);
        1.0 - 10f64.powf(-(value * 0.0015258789) / 20.0)
    }

    fn frequency(value: f64) -> f64 {
        let hz = 2f64.powf(value) * 32.703197;
        (hz * std::f64::consts::PI) / 11025.0
    }

    fn frequency_for(&self, direction: usize, pair: usize, t: f64) -> f64 {
        let base = self.frequencies[direction][0][pair] as f64;
        let range = self.frequencies[direction][1][pair] as f64;
        let value = base + t * (range - base);
        Self::frequency(value * 1.2207031e-4)
    }

    /// `calculateCoeffs(direction, t)`: returns the coefficient count and
    /// leaves `reduceCoeffInt` set when `direction == 0`.
    pub fn calculate_coeffs(&mut self, direction: usize, t: f64) -> i32 {
        if direction == 0 {
            let unity = self.unities[0] as f64 + (self.unities[1] - self.unities[0]) as f64 * t;
            let scaled = unity * 0.0030517578;
            self.reduce_coeff = 0.1f64.powf(scaled / 20.0);
            self.reduce_coeff_int = (self.reduce_coeff * 65536.0) as i32;
        }

        if self.pairs[direction] == 0 {
            return 0;
        }

        let mut r = self.radius(0, direction, t);
        self.coeff[direction][0] = -2.0 * r * self.frequency_for(direction, 0, t).cos();
        self.coeff[direction][1] = r * r;

        for pair in 1..self.pairs[direction] as usize {
            r = self.radius(pair, direction, t);
            let coeff = -2.0 * r * self.frequency_for(direction, pair, t).cos();
            let coeff2 = r * r;

            self.coeff[direction][pair * 2 + 1] = self.coeff[direction][pair * 2 - 1] * coeff2;
            self.coeff[direction][pair * 2] =
                self.coeff[direction][pair * 2 - 1] * coeff + self.coeff[direction][pair * 2 - 2] * coeff2;

            let mut i = pair * 2 - 1;
            while i >= 2 {
                self.coeff[direction][i] +=
                    self.coeff[direction][i - 1] * coeff + self.coeff[direction][i - 2] * coeff2;
                i -= 1;
            }

            self.coeff[direction][1] += self.coeff[direction][0] * coeff + coeff2;
            self.coeff[direction][0] += coeff;
        }

        if direction == 0 {
            let count = self.pairs[0] * 2;
            for i in 0..count as usize {
                self.coeff[0][i] *= self.reduce_coeff;
            }
        }

        let count = self.pairs[direction] * 2;
        for i in 0..count as usize {
            self.coeff_int[direction][i] = (self.coeff[direction][i] * 65536.0) as i32;
        }

        count
    }

    fn coeff_int(&self, direction: usize, index: usize) -> i32 {
        self.coeff_int[direction][index]
    }

    fn reduce_coeff_int(&self) -> i32 {
        self.reduce_coeff_int
    }
}

/// `Tone.ts`: one voice of a `JagFX` sound.
#[derive(Clone)]
pub struct Tone {
    frequency_base: Envelope,
    amplitude_base: Envelope,

    frequency_mod_rate: Option<Envelope>,
    frequency_mod_range: Option<Envelope>,

    amplitude_mod_rate: Option<Envelope>,
    amplitude_mod_range: Option<Envelope>,

    release: Option<Envelope>,
    attack: Option<Envelope>,

    harmonic_volume: [i32; 5],
    harmonic_semitone: [i32; 5],
    harmonic_delay: [i32; 5],

    reverb_delay: i32,
    reverb_volume: i32,

    pub length: i32,
    pub start: i32,

    filter: Filter,
    filter_range: Envelope,

    f_pos: [i32; 5],
    f_del: [i32; 5],
    f_amp: [i32; 5],
    f_multi: [i32; 5],
    f_offset: [i32; 5],
}

impl Tone {
    /// `load(buf)` from TS.
    pub fn load(buf: &mut Packet) -> Self {
        let mut tone = Tone {
            frequency_base: Envelope::load(buf),
            amplitude_base: Envelope::load(buf),
            frequency_mod_rate: None,
            frequency_mod_range: None,
            amplitude_mod_rate: None,
            amplitude_mod_range: None,
            release: None,
            attack: None,
            harmonic_volume: [0; 5],
            harmonic_semitone: [0; 5],
            harmonic_delay: [0; 5],
            reverb_delay: 0,
            reverb_volume: 100,
            length: 500,
            start: 0,
            filter: Filter::default(),
            filter_range: Envelope::new(),
            f_pos: [0; 5],
            f_del: [0; 5],
            f_amp: [0; 5],
            f_multi: [0; 5],
            f_offset: [0; 5],
        };

        if buf.g1() != 0 {
            buf.pos -= 1;
            tone.frequency_mod_rate = Some(Envelope::load(buf));
            tone.frequency_mod_range = Some(Envelope::load(buf));
        }

        if buf.g1() != 0 {
            buf.pos -= 1;
            tone.amplitude_mod_rate = Some(Envelope::load(buf));
            tone.amplitude_mod_range = Some(Envelope::load(buf));
        }

        if buf.g1() != 0 {
            buf.pos -= 1;
            tone.release = Some(Envelope::load(buf));
            tone.attack = Some(Envelope::load(buf));
        }

        // the TS load loop reads up to 10 harmonics but its `harmonicVolume`
        // array is length 5, so slots 5-9 are consumed and dropped
        for harmonic in 0..10 {
            let volume = buf.gsmart();
            if volume == 0 {
                break;
            }
            if harmonic < 5 {
                tone.harmonic_volume[harmonic] = volume;
                tone.harmonic_semitone[harmonic] = buf.gsmarts();
                tone.harmonic_delay[harmonic] = buf.gsmart();
            } else {
                let _semitone = buf.gsmarts();
                let _delay = buf.gsmart();
            }
        }

        tone.reverb_delay = buf.gsmart();
        tone.reverb_volume = buf.gsmart();
        tone.length = buf.g2();
        tone.start = buf.g2();

        tone.filter.unpack(buf, &mut tone.filter_range);

        tone
    }

    /// `generate(sampleCount, length)` from TS; `scratch` is the shared
    /// `Tone.buf` (22050 * 10 samples) and holds the result.
    pub fn generate<'a>(&mut self, sample_count: i32, length: i32, scratch: &'a mut [i32]) -> &'a [i32] {
        for sample in 0..sample_count as usize {
            scratch_set(scratch, sample, 0);
        }

        if length < 10 {
            return scratch;
        }

        let samples_per_step = sample_count as f64 / length as f64;

        self.frequency_base.gen_init();
        self.amplitude_base.gen_init();

        let mut frequency_start = 0i32;
        let mut frequency_duration = 0i32;
        let mut frequency_form = 0i32;
        if let (Some(rate), Some(range)) = (&mut self.frequency_mod_rate, &mut self.frequency_mod_range) {
            rate.gen_init();
            range.gen_init();
            frequency_form = rate.form;
            frequency_start = (((rate.end - rate.start) as f64 * 32.768) / samples_per_step) as i32;
            frequency_duration = (rate.start as f64 * 32.768 / samples_per_step) as i32;
        }

        let mut amplitude_start = 0i32;
        let mut amplitude_duration = 0i32;
        let mut amplitude_form = 0i32;
        if let (Some(rate), Some(range)) = (&mut self.amplitude_mod_rate, &mut self.amplitude_mod_range) {
            rate.gen_init();
            range.gen_init();
            amplitude_form = rate.form;
            amplitude_start = (((rate.end - rate.start) as f64 * 32.768) / samples_per_step) as i32;
            amplitude_duration = (rate.start as f64 * 32.768 / samples_per_step) as i32;
        }

        for harmonic in 0..5 {
            if self.harmonic_volume[harmonic] != 0 {
                self.f_pos[harmonic] = 0;
                self.f_del[harmonic] = (self.harmonic_delay[harmonic] as f64 * samples_per_step) as i32;
                self.f_amp[harmonic] = (self.harmonic_volume[harmonic] << 14) / 100;
                self.f_multi[harmonic] = (((self.frequency_base.end - self.frequency_base.start) as f64
                    * 32.768
                    * 1.0057929410678534f64.powf(self.harmonic_semitone[harmonic] as f64))
                    / samples_per_step) as i32;
                self.f_offset[harmonic] = (self.frequency_base.start as f64 * 32.768 / samples_per_step) as i32;
            }
        }

        let mut frequency_phase = 0i32;
        let mut amplitude_phase = 0i32;
        for sample in 0..sample_count as usize {
            let mut frequency = self.frequency_base.gen_next(sample_count);
            let mut amplitude = self.amplitude_base.gen_next(sample_count);

            if let (Some(rate), Some(range)) = (&mut self.frequency_mod_rate, &mut self.frequency_mod_range) {
                let rate = rate.gen_next(sample_count);
                let range = range.gen_next(sample_count);
                frequency += self.wave_func(range, frequency_phase, frequency_form) >> 1;
                frequency_phase = frequency_phase.wrapping_add(
                    ((rate.wrapping_mul(frequency_start)) >> 16).wrapping_add(frequency_duration),
                );
            }

            if let (Some(rate), Some(range)) = (&mut self.amplitude_mod_rate, &mut self.amplitude_mod_range) {
                let rate = rate.gen_next(sample_count);
                let range = range.gen_next(sample_count);
                amplitude = (amplitude.wrapping_mul(
                    (self.wave_func(range, amplitude_phase, amplitude_form) >> 1).wrapping_add(32768),
                )) >> 15;
                amplitude_phase = amplitude_phase.wrapping_add(
                    ((rate.wrapping_mul(amplitude_start)) >> 16).wrapping_add(amplitude_duration),
                );
            }

            for harmonic in 0..5 {
                if self.harmonic_volume[harmonic] != 0 {
                    let position = sample as i32 + self.f_del[harmonic];
                    if position < sample_count {
                        let value = self.wave_func(
                            (amplitude.wrapping_mul(self.f_amp[harmonic])) >> 15,
                            self.f_pos[harmonic],
                            self.frequency_base.form,
                        );
                        scratch_add(scratch, position as usize, value);
                        self.f_pos[harmonic] = self.f_pos[harmonic].wrapping_add(
                            ((frequency.wrapping_mul(self.f_multi[harmonic])) >> 16).wrapping_add(self.f_offset[harmonic]),
                        );
                    }
                }
            }
        }

        if let (Some(release), Some(attack)) = (&mut self.release, &mut self.attack) {
            release.gen_init();
            attack.gen_init();

            let mut counter = 0i32;
            let mut muted = true;

            for sample in 0..sample_count as usize {
                let release_value = release.gen_next(sample_count);
                let attack_value = attack.gen_next(sample_count);

                let threshold: i32 = if muted {
                    release.start + ((((release.end as i64 - release.start as i64) * release_value as i64) >> 8) as i32)
                } else {
                    release.start + ((((release.end as i64 - release.start as i64) * attack_value as i64) >> 8) as i32)
                };

                counter = counter.wrapping_add(256);
                if counter >= threshold {
                    counter = 0;
                    muted = !muted;
                }

                if muted {
                    scratch_set(scratch, sample, 0);
                }
            }
        }

        if self.reverb_delay > 0 && self.reverb_volume > 0 {
            let start = (self.reverb_delay as f64 * samples_per_step) as i32;

            for sample in start as usize..sample_count as usize {
                let value = ((scratch_get(scratch, sample - start as usize) as f64 * self.reverb_volume as f64) / 100.0) as i32;
                scratch_add(scratch, sample, value);
            }
        }

        if self.filter.pairs[0] > 0 || self.filter.pairs[1] > 0 {
            self.filter_range.gen_init();

            let mut range = self.filter_range.gen_next(sample_count + 1);
            let mut coeff0 = self.filter.calculate_coeffs(0, range as f64 / 65536.0);
            let mut coeff1 = self.filter.calculate_coeffs(1, range as f64 / 65536.0);

            if sample_count >= coeff0 + coeff1 {
                let mut sample = 0i32;
                let mut limit = coeff1;

                if coeff1 > sample_count - coeff0 {
                    limit = sample_count - coeff0;
                }

                while sample < limit {
                    let mut value = mul_shift16(scratch_get(scratch, (sample + coeff0) as usize), self.filter.reduce_coeff_int());

                    for i in 0..coeff0 {
                        value = value.wrapping_add(mul_shift16(
                            scratch_get(scratch, (sample + coeff0 - i - 1) as usize),
                            self.filter.coeff_int(0, i as usize),
                        ));
                    }

                    for i in 0..sample {
                        value = value.wrapping_sub(mul_shift16(
                            scratch_get(scratch, (sample - i - 1) as usize),
                            self.filter.coeff_int(1, i as usize),
                        ));
                    }

                    scratch_set(scratch, sample as usize, value);
                    range = self.filter_range.gen_next(sample_count + 1);
                    sample += 1;
                }

                const STEP: i32 = 128;
                let mut next = STEP;

                loop {
                    if next > sample_count - coeff0 {
                        next = sample_count - coeff0;
                    }

                    while sample < next {
                        let mut value = mul_shift16(scratch_get(scratch, (sample + coeff0) as usize), self.filter.reduce_coeff_int());

                        for i in 0..coeff0 {
                            value = value.wrapping_add(mul_shift16(
                                scratch_get(scratch, (sample + coeff0 - i - 1) as usize),
                                self.filter.coeff_int(0, i as usize),
                            ));
                        }

                        for i in 0..coeff1 {
                            value = value.wrapping_sub(mul_shift16(
                                scratch_get(scratch, (sample - i - 1) as usize),
                                self.filter.coeff_int(1, i as usize),
                            ));
                        }

                        scratch_set(scratch, sample as usize, value);
                        range = self.filter_range.gen_next(sample_count + 1);
                        sample += 1;
                    }

                    if sample >= sample_count - coeff0 {
                        while sample < sample_count {
                            let mut value = 0i32;

                            for i in (sample + coeff0 - sample_count)..coeff0 {
                                value = value.wrapping_add(mul_shift16(
                                    scratch_get(scratch, (sample + coeff0 - i - 1) as usize),
                                    self.filter.coeff_int(0, i as usize),
                                ));
                            }

                            for i in 0..coeff1 {
                                value = value.wrapping_sub(mul_shift16(
                                    scratch_get(scratch, (sample - i - 1) as usize),
                                    self.filter.coeff_int(1, i as usize),
                                ));
                            }

                            scratch_set(scratch, sample as usize, value);
                            self.filter_range.gen_next(sample_count + 1);
                            sample += 1;
                        }
                        break;
                    }

                    coeff0 = self.filter.calculate_coeffs(0, range as f64 / 65536.0);
                    coeff1 = self.filter.calculate_coeffs(1, range as f64 / 65536.0);
                    next += STEP;
                }
            }
        }

        for v in scratch.iter_mut().take(sample_count as usize) {
            *v = (*v).clamp(-32768, 32767);
        }

        scratch
    }

    /// `waveFunc(amplitude, phase, form)` from TS.
    fn wave_func(&self, amplitude: i32, phase: i32, form: i32) -> i32 {
        match form {
            1 => {
                if phase & 0x7fff < 16384 {
                    amplitude
                } else {
                    -amplitude
                }
            }
            2 => (sine_table()[(phase & 0x7fff) as usize].wrapping_mul(amplitude)) >> 14,
            3 => (((phase & 0x7fff).wrapping_mul(amplitude)) >> 14) - amplitude,
            4 => noise_table()[((phase as f64 / 2607.0) as i32 & 0x7fff) as usize].wrapping_mul(amplitude),
            _ => 0,
        }
    }
}

/// `mulShift16(a, b)` from JsUtil: `((a * b) >> 16) | 0`.
#[inline]
fn mul_shift16(a: i32, b: i32) -> i32 {
    a.wrapping_mul(b) >> 16
}
