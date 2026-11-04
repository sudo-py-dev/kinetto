fn main() -> eframe::Result {
    tracing_subscriber::fmt::init();
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
