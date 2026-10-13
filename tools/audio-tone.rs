//! Bounded synthetic audio fixture for the private test server, never the desktop output.
use std::{env, fs::File, io::{self, Write}};
fn main() -> io::Result<()> {
    let args: Vec<_> = env::args().skip(1).collect();
    if args == ["--version"] { println!("g13pad-audio-tone 0.2.79"); return Ok(()); }
    let [path] = args.as_slice() else { return Err(io::Error::other("usage: audio-tone FILE.f32le")); };
    let mut file = File::create(path)?;
    // Eight seconds of alternating voice-sized tone bursts and silence, peak0.1.
    let mut block = [0u8;4800*4];
    for tenth in 0..80 {
        for (i, bytes) in block.chunks_exact_mut(4).enumerate() {
            let t = (tenth*4800+i) as f32/48000.0;
            let sample = if tenth%10 < 6 { 0.1*(t*440.0*std::f32::consts::TAU).sin() } else { 0.0 };
            bytes.copy_from_slice(&sample.to_le_bytes());
        }
        file.write_all(&block)?;
    }
    Ok(())
}
