//! Plays five seconds of silence with every core busy, then reports how the
//! sound-card clock kept up and whether the device ever ran dry.

fn main() {
    let Some(player) = kn_ui::device::Player::new() else {
        println!("no sound device");
        return;
    };
    let silence = std::sync::Arc::new(kn_audio::Pcm { samples: vec![0.0; 48_000 * 2 * 8] });
    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let busy: Vec<_> = (0..std::thread::available_parallelism().map_or(4, |n| n.get()))
        .map(|_| {
            let stop = stop.clone();
            std::thread::spawn(move || {
                let mut x = 0u64;
                while !stop.load(std::sync::atomic::Ordering::Relaxed) {
                    x = x.wrapping_mul(6364136223846793005).wrapping_add(1);
                }
                x
            })
        })
        .collect();
    player.play(silence, 0);
    let start = std::time::Instant::now();
    std::thread::sleep(std::time::Duration::from_secs(5));
    let (glitches, latency) = player.health();
    println!("wall {:.3} s, sound clock {:.3} s, latency {:?}, underruns {glitches}", start.elapsed().as_secs_f64(), player.position_secs(), latency);
    stop.store(true, std::sync::atomic::Ordering::Relaxed);
    busy.into_iter().for_each(|t| {
        t.join().ok();
    });
}
