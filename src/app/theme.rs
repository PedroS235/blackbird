use egui::{FontData, FontDefinitions, FontFamily, FontId, TextStyle};

pub fn install(ctx: &egui::Context) {
    install_fonts(ctx);
}

/// Installs the elegance palette matching `dark` — charcoal/paper are the
/// crate's own dark/light pair, kept pixel-identical apart from luminance.
/// Called every frame: `Theme::install` is a no-op once the palette already
/// matches, and the app's resolved light/dark state can change at any time
/// (system theme switch, or the sidepanel toggle).
pub fn apply(ctx: &egui::Context, dark: bool) {
    if dark {
        elegance::Theme::charcoal()
    } else {
        elegance::Theme::paper()
    }
    .install(ctx);

    // elegance sets text styles to plain Proportional; re-point Heading and
    // Button at the heavier Inter weights we loaded above.
    ctx.global_style_mut(|style| {
        style.text_styles.insert(
            TextStyle::Heading,
            FontId::new(19.0, FontFamily::Name("semibold".into())),
        );
        style.text_styles.insert(
            TextStyle::Button,
            FontId::new(14.0, FontFamily::Name("medium".into())),
        );
    });
}

fn install_fonts(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();

    fonts.font_data.insert(
        "inter-regular".to_owned(),
        FontData::from_static(include_bytes!("../../assets/fonts/Inter-Regular.ttf")).into(),
    );
    fonts.font_data.insert(
        "inter-medium".to_owned(),
        FontData::from_static(include_bytes!("../../assets/fonts/Inter-Medium.ttf")).into(),
    );
    fonts.font_data.insert(
        "inter-semibold".to_owned(),
        FontData::from_static(include_bytes!("../../assets/fonts/Inter-SemiBold.ttf")).into(),
    );

    fonts
        .families
        .entry(FontFamily::Proportional)
        .or_default()
        .insert(0, "inter-regular".to_owned());
    fonts.families.insert(
        FontFamily::Name("medium".into()),
        vec!["inter-medium".to_owned()],
    );
    fonts.families.insert(
        FontFamily::Name("semibold".into()),
        vec!["inter-semibold".to_owned()],
    );

    egui_phosphor::add_to_fonts(&mut fonts, egui_phosphor::Variant::Regular);

    ctx.set_fonts(fonts);
}
