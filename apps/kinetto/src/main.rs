fn main() -> eframe::Result {
    // Our own messages at info; libraries only when something is wrong.
    // `RUST_LOG` overrides this, e.g. `RUST_LOG=debug`.
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn,kn_ui=info,kinetto=info,calloop=error"));
    tracing_subscriber::fmt().with_env_filter(filter).init();
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("Kinetto")
            .with_inner_size([1280.0, 800.0])
            .with_min_inner_size([900.0, 560.0])
            // Kinetto draws its own header bar and rounded frame.
            .with_decorations(false)
            .with_transparent(true)
            .with_app_id("kinetto"),
        renderer: eframe::Renderer::Wgpu,
        ..Default::default()
    };
    eframe::run_native("Kinetto", options, Box::new(|cc| Ok(Box::new(kn_ui::KinettoApp::new(cc)))))
}
