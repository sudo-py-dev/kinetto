//! Plays half a second of silence and reports how far the sound-card clock
//! moved, to check the device path works.

fn main() {
    let Some(player) = kn_ui::device::Player::new() else {
        println!("no sound device");
        return;
    };
    let silence = std::sync::Arc::new(kn_audio::Pcm { samples: vec![0.0; 48_000 * 2 * 2] });
    player.play(silence, 0);
    let start = std::time::Instant::now();
    std::thread::sleep(std::time::Duration::from_millis(500));
    let moved = player.position_secs();
    player.stop();
    println!("wall {:.3} s, sound clock {:.3} s", start.elapsed().as_secs_f64(), moved);
}
