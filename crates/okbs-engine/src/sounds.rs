//! Built-in sounds, synthesized at first use (no audio files to license).

use std::sync::OnceLock;

/// Events that can play a sound («Звуки»).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Sound {
    /// Automatic layout switch.
    Autoswitch,
    /// Conversion by hotkey.
    ManualConvert,
    /// Layout changed by the user.
    LayoutChanged,
    /// Conversion undone.
    Cancel,
    /// Possible typo.
    Suspicious,
    /// Autoreplace performed.
    Autoreplace,
    /// Case corrected.
    CaseFixed,
    /// Clipboard converted.
    ClipboardConvert,
    /// Operation failed.
    Error,
}

const RATE: u32 = 22_050;

/// `(frequency Hz, duration ms)` segments of each sound.
fn melody(sound: Sound) -> &'static [(f32, u32)] {
    match sound {
        Sound::Autoswitch => &[(880.0, 35), (1320.0, 45)],
        Sound::ManualConvert => &[(1175.0, 40)],
        Sound::LayoutChanged => &[(988.0, 25)],
        Sound::Cancel => &[(1320.0, 35), (880.0, 45)],
        Sound::Suspicious => &[(330.0, 90)],
        Sound::Autoreplace => &[(1047.0, 30), (1397.0, 30), (1760.0, 40)],
        Sound::CaseFixed => &[(1568.0, 35)],
        Sound::ClipboardConvert => &[(784.0, 30), (1047.0, 40)],
        Sound::Error => &[(220.0, 60), (0.0, 30), (220.0, 60)],
    }
}

fn synthesize(sound: Sound) -> Vec<u8> {
    let mut samples: Vec<i16> = Vec::new();
    for &(freq, ms) in melody(sound) {
        let n = RATE * ms / 1000;
        let fade = (RATE / 200).max(1).min(n / 2).max(1);
        for i in 0..n {
            let t = i as f32 / RATE as f32;
            let envelope = (i.min(n - 1 - i) as f32 / fade as f32).min(1.0);
            let value = if freq > 0.0 {
                (t * freq * std::f32::consts::TAU).sin() * envelope * 0.35
            } else {
                0.0
            };
            samples.push((value * f32::from(i16::MAX)) as i16);
        }
    }
    let data_len = (samples.len() * 2) as u32;
    let mut wav = Vec::with_capacity(44 + data_len as usize);
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36 + data_len).to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&RATE.to_le_bytes());
    wav.extend_from_slice(&(RATE * 2).to_le_bytes());
    wav.extend_from_slice(&2u16.to_le_bytes());
    wav.extend_from_slice(&16u16.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&data_len.to_le_bytes());
    for s in samples {
        wav.extend_from_slice(&s.to_le_bytes());
    }
    wav
}

/// WAV data (PCM 16-bit mono) of a built-in sound.
pub fn wav(sound: Sound) -> &'static [u8] {
    const ALL: [Sound; 9] = [
        Sound::Autoswitch,
        Sound::ManualConvert,
        Sound::LayoutChanged,
        Sound::Cancel,
        Sound::Suspicious,
        Sound::Autoreplace,
        Sound::CaseFixed,
        Sound::ClipboardConvert,
        Sound::Error,
    ];
    static CACHE: OnceLock<Vec<Vec<u8>>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| ALL.iter().map(|&s| synthesize(s)).collect());
    let index = ALL.iter().position(|&s| s == sound).unwrap_or(0);
    &cache[index]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wav_headers() {
        let data = wav(Sound::Autoswitch);
        assert_eq!(&data[0..4], b"RIFF");
        assert_eq!(&data[8..16], b"WAVEfmt ");
        let data_len = u32::from_le_bytes([data[40], data[41], data[42], data[43]]) as usize;
        assert_eq!(data.len(), 44 + data_len);
        let samples: u32 = melody(Sound::Autoswitch)
            .iter()
            .map(|&(_, ms)| RATE * ms / 1000)
            .sum();
        assert_eq!(data_len, samples as usize * 2);
        assert!(data[44..].iter().any(|&b| b != 0));
        assert!(std::ptr::eq(wav(Sound::Error), wav(Sound::Error)));
    }
}
