//! Render a minute of every generated mood to WAV files for listening:
//! `cargo run --release -p kn-audio --example moods -- <out dir>`

use kn_audio::{RATE, generate};
use kn_model::Mood;
use std::time::Instant;

fn main() {
    let dir = std::env::args().nth(1).unwrap_or_else(|| "moods".into());
    std::fs::create_dir_all(&dir).expect("output folder");
    for mood in Mood::ALL {
        let began = Instant::now();
        let pcm = generate(mood, RATE as usize * 60);
        let path = format!("{dir}/{}.wav", mood.name().to_lowercase());
        std::fs::write(&path, pcm.to_wav()).expect("write wav");
        println!("{path} in {:.2?}", began.elapsed());
    }
}
