//! Audio output. One track at a time, decoded from memory so seeking and
//! replay need no network.
//!
//! `OMADORA_AUDIO=null` swaps the sound device for a sink that consumes
//! samples at real-time speed, for testing without making noise.

use anyhow::{Context, Result};
use rodio::{Decoder, Player, Source};
use std::io::Cursor;
use std::sync::Arc;
use std::time::Duration;

pub struct Audio {
    player: Player,
    // Dropping the device sink stops playback, so it lives as long as Audio.
    _device: Option<rodio::MixerDeviceSink>,
}

impl Audio {
    pub fn open() -> Result<Self> {
        if std::env::var("OMADORA_AUDIO").as_deref() == Ok("null") {
            let (player, output) = Player::new();
            std::thread::spawn(move || drain(output));
            return Ok(Self { player, _device: None });
        }
        let mut device = rodio::DeviceSinkBuilder::open_default_sink().context("opening the audio device")?;
        device.log_on_drop(false);
        let player = Player::connect_new(device.mixer());
        Ok(Self { player, _device: Some(device) })
    }

    /// Replace whatever is playing with this track, paused at the start.
    pub fn load(&self, bytes: Arc<Vec<u8>>, encoding: &str, gain_db: f32) -> Result<()> {
        let len = bytes.len() as u64;
        let decoder = Decoder::builder()
            .with_data(Cursor::new(SharedBytes(bytes)))
            .with_byte_len(len)
            .with_seekable(true)
            .with_hint(if encoding.is_empty() { "mp3" } else { encoding })
            .build()
            .context("decoding the track")?;
        self.player.clear();
        self.player.append(decoder.amplify(db_to_gain(gain_db)));
        Ok(())
    }

    pub fn play(&self) {
        self.player.play();
    }

    pub fn pause(&self) {
        self.player.pause();
    }

    pub fn stop(&self) {
        self.player.clear();
    }

    pub fn is_paused(&self) -> bool {
        self.player.is_paused()
    }

    /// True once the loaded track has played to its end.
    pub fn finished(&self) -> bool {
        self.player.empty()
    }

    pub fn seek(&self, to: Duration) -> Result<()> {
        self.player.try_seek(to).map_err(|e| anyhow::anyhow!("seek failed: {e}"))
    }

    pub fn set_volume(&self, volume: f32) {
        self.player.set_volume(volume.clamp(0.0, 1.0) as _);
    }

    // rodio's Float is f64 under its `64bit` feature.
    #[allow(clippy::unnecessary_cast)]
    pub fn volume(&self) -> f32 {
        self.player.volume() as f32
    }
}

/// Radio comes as HE-AAC ("aacplus"), whose high band (SBR) no Rust decoder
/// reconstructs. ffmpeg decodes it fully to raw PCM, wrapped here in a WAV
/// header (ffmpeg cannot write a WAV's sizes into a pipe), which plays and
/// seeks like any other track.
pub fn decode_with_ffmpeg(bytes: &[u8]) -> Result<Vec<u8>> {
    use std::io::{Read, Write};
    use std::process::{Command, Stdio};
    let mut child = Command::new("ffmpeg")
        .args(["-v", "error", "-i", "pipe:0", "-f", "s16le", "-acodec", "pcm_s16le"])
        .args(["-ar", &PCM_RATE.to_string(), "-ac", &PCM_CHANNELS.to_string(), "pipe:1"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("running ffmpeg to decode radio audio")?;
    let mut stdin = child.stdin.take().expect("piped stdin");
    let input = bytes.to_vec();
    // Feed and drain at once, or a full pipe would stall both sides.
    let feeder = std::thread::spawn(move || {
        let _ = stdin.write_all(&input);
    });
    let mut out = Vec::new();
    child.stdout.take().expect("piped stdout").read_to_end(&mut out)?;
    let _ = feeder.join();
    let status = child.wait()?;
    if !status.success() || out.is_empty() {
        let mut err = String::new();
        if let Some(mut e) = child.stderr.take() {
            let _ = e.read_to_string(&mut err);
        }
        anyhow::bail!("ffmpeg could not decode the track: {}", err.trim());
    }
    Ok(wav(out))
}

const PCM_RATE: u32 = 44_100;
const PCM_CHANNELS: u16 = 2;

/// A WAV file around 16-bit little-endian PCM.
fn wav(pcm: Vec<u8>) -> Vec<u8> {
    let data_len = pcm.len() as u32;
    let block_align = PCM_CHANNELS * 2;
    let mut out = Vec::with_capacity(pcm.len() + 44);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&PCM_CHANNELS.to_le_bytes());
    out.extend_from_slice(&PCM_RATE.to_le_bytes());
    out.extend_from_slice(&(PCM_RATE * block_align as u32).to_le_bytes());
    out.extend_from_slice(&block_align.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    out.extend(pcm);
    out
}

/// Pandora sends a per-track gain in dB to even out loudness.
fn db_to_gain(db: f32) -> f32 {
    10f32.powf(db / 20.0)
}

/// Lets several decoders share one downloaded file without copying it.
struct SharedBytes(Arc<Vec<u8>>);

impl AsRef<[u8]> for SharedBytes {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

/// The null device: pull samples at the rate a sound card would.
fn drain(mut output: rodio::queue::SourcesQueueOutput) {
    let tick = Duration::from_millis(20);
    loop {
        let rate = output.sample_rate().get() as u64;
        let channels = output.channels().get() as u64;
        let per_tick = (rate * channels * tick.as_millis() as u64 / 1000).max(1);
        for _ in 0..per_tick {
            output.next();
        }
        std::thread::sleep(tick);
    }
}

#[cfg(test)]
mod tests {
    use rodio::{Decoder, Source};
    use std::io::Cursor;

    #[test]
    fn wav_wrapper_decodes() {
        // One second of silence, stereo 16-bit.
        let pcm = vec![0u8; super::PCM_RATE as usize * 4];
        let bytes = super::wav(pcm);
        let len = bytes.len() as u64;
        let decoder = Decoder::builder().with_data(Cursor::new(bytes)).with_byte_len(len).with_hint("wav").build().unwrap();
        assert_eq!(decoder.sample_rate().get(), super::PCM_RATE);
        assert_eq!(decoder.channels().get(), 2);
        assert_eq!(decoder.count(), super::PCM_RATE as usize * 2);
    }

    #[test]
    fn ffmpeg_round_trip() {
        // A short AAC file made by ffmpeg itself, decoded back through it.
        let made = std::process::Command::new("ffmpeg")
            .args(["-v", "error", "-f", "lavfi", "-i", "sine=frequency=440:duration=1", "-c:a", "aac", "-f", "adts", "pipe:1"])
            .output()
            .unwrap();
        let wav = super::decode_with_ffmpeg(&made.stdout).unwrap();
        let len = wav.len() as u64;
        let decoder = Decoder::builder().with_data(Cursor::new(wav)).with_byte_len(len).with_hint("wav").build().unwrap();
        let seconds = decoder.count() as f64 / super::PCM_RATE as f64 / 2.0;
        assert!((0.9..1.2).contains(&seconds), "{seconds}");
    }
}
