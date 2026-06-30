pub fn load_texture_from_bytes(
    ctx: &egui::Context,
    bytes: Vec<u8>,
    texture: &mut Option<egui::TextureHandle>,
) {
    let image = image::load_from_memory(&bytes).unwrap().to_rgba8();

    let size = [image.width() as usize, image.height() as usize];

    let color_image = egui::ColorImage::from_rgba_unmultiplied(size, image.as_raw());

    *texture = Some(ctx.load_texture("captcha_original", color_image, Default::default()));
}
