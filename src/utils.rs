use rand::distr::{Alphanumeric, SampleString};

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

pub fn rand_str(length: usize) -> String {
    let random_string = Alphanumeric.sample_string(&mut rand::rng(), 16);

    random_string
}
