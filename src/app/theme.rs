use egui::{FontData, FontDefinitions, FontFamily, FontId, TextStyle};

pub fn install(ctx: &egui::Context) {
    install_fonts(ctx);
    elegance::Theme::slate().install(ctx);

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

    ctx.set_fonts(fonts);
}
