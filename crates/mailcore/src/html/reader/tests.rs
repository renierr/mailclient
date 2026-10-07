use super::*;

fn inverted(hex: &str) -> String {
    Rgb::parse(hex).unwrap().inverted().css()
}

fn options(paint: Paint) -> DocumentOptions<'static> {
    DocumentOptions {
        paint,
        theme: Palette {
            paper: Rgb(0x16181D),
            ink: Rgb(0xE6E6E6),
            link: Rgb(0x3B82F6),
            quote: Rgb(0x9AA0A6),
            rule: Rgb(0x2A2D33),
        },
        allow_remote: false,
        top_space: 0,
        scale: 1.0,
        fit: false,
        extra_css: "",
    }
}

fn csp(doc: &str) -> &str {
    let start = doc.find("Content-Security-Policy\" content=\"").unwrap() + 34;
    let len = doc[start..].find('"').unwrap();
    &doc[start..start + len]
}

#[test]
fn a_mail_without_colours_always_takes_the_theme() {
    for dark in [true, false] {
        assert_eq!(paint_for(false, dark, false), Paint::Theme);
    }
}

#[test]
fn a_designed_mail_darkens_in_a_dark_theme_unless_kept_original() {
    assert_eq!(paint_for(true, true, false), Paint::Darkened);
    assert_eq!(paint_for(true, true, true), Paint::Original);
    assert_eq!(paint_for(true, false, false), Paint::Original);
    for p in [Paint::Theme, Paint::Original, Paint::Darkened] {
        assert_eq!(Paint::parse(p.as_str()), p);
    }
}

#[test]
fn inverting_flips_lightness_and_twice_gives_the_colour_back() {
    assert_eq!(Rgb(0xFFFFFF).inverted(), Rgb(0x000000));
    let grey = Rgb(0x333333).inverted();
    assert_eq!(grey.0 >> 16, 0xFF - 0x33);
    let c = Rgb(0x3B82F6);
    let back = c.inverted().inverted();
    for shift in [16, 8, 0] {
        let (a, b) = ((c.0 >> shift) & 0xFF, (back.0 >> shift) & 0xFF);
        assert!(a.abs_diff(b) <= 3, "{c:?} -> {back:?}");
    }
}

#[test]
fn a_theme_colour_that_does_not_parse_keeps_the_light_one() {
    let p = Palette::from_css("#16181d", "nonsense", "#3b82f6", "", "#2a2d33");
    assert_eq!(p.paper, Rgb(0x16181D));
    assert_eq!(p.ink, LIGHT.ink);
    assert_eq!(p.quote, LIGHT.quote);
}

#[test]
fn only_plain_colours_parse() {
    assert_eq!(Rgb::parse("#abc"), Some(Rgb(0xAABBCC)));
    assert_eq!(Rgb::parse(" #16181d "), Some(Rgb(0x16181D)));
    for bad in ["", "#12", "#1234", "#ff00ff80", "red", "#fff;}body{"] {
        assert_eq!(Rgb::parse(bad), None, "{bad}");
    }
}

#[test]
fn darkening_inverts_hex_colours_with_the_filter_matrix() {
    assert_eq!(
        darken_colors(r#"<p style="color:#ffffff;">x</p>"#),
        format!(r#"<p style="color:{};">x</p>"#, inverted("#ffffff"))
    );
    assert_eq!(
        darken_colors(r#"<div style="background-color:#000000">x</div>"#),
        format!(
            r#"<div style="background-color:{}">x</div>"#,
            inverted("#000000")
        )
    );
}

#[test]
fn darkening_converts_keywords_rgb_and_attributes() {
    assert_eq!(
        darken_colors(r#"<p style="color:red;">x</p>"#),
        format!(r#"<p style="color:{};">x</p>"#, inverted("#ff0000"))
    );
    assert_eq!(
        darken_colors(r#"<p style="color:rgb(255, 0, 0);">x</p>"#),
        format!(r#"<p style="color:{};">x</p>"#, inverted("#ff0000"))
    );
    assert_eq!(
        darken_colors(r#"<td bgcolor="navy">x</td>"#),
        format!(r#"<td bgcolor="{}">x</td>"#, inverted("#000080"))
    );
    assert_eq!(
        darken_colors(r##"<font color="#123456">x</font>"##),
        format!(r#"<font color="{}">x</font>"#, inverted("#123456"))
    );
}

#[test]
fn darkening_keeps_alpha_and_important() {
    let out = darken_colors(r#"<p style="color:rgba(255,255,255,0.5)!important;">x</p>"#);
    assert!(out.contains("rgba("), "{out}");
    // 0.5 rounds through 8-bit alpha (128) and back.
    assert!(out.contains("0.502)!important"), "{out}");
    assert!(!out.contains("255,255,255"), "{out}");
}

#[test]
fn darkening_converts_the_colour_inside_border_shorthands() {
    assert_eq!(
        darken_colors(r##"<td style="border:1px solid #dddddd;">x</td>"##),
        format!(
            r#"<td style="border:1px solid {};">x</td>"#,
            inverted("#dddddd")
        )
    );
}

#[test]
fn darkening_converts_both_bgcolor_and_color_on_one_element() {
    assert_eq!(
        darken_colors(r##"<td bgcolor="#ffffff" color="#000000">x</td>"##),
        format!(
            r#"<td bgcolor="{}" color="{}">x</td>"#,
            inverted("#ffffff"),
            inverted("#000000")
        )
    );
}

#[test]
fn darkening_leaves_the_rest_of_the_mail_alone() {
    let html = concat!(
        r#"<table width="600">"#,
        r#"<tr><td style="padding:4px;width:600px;">"#,
        r#"width="600" and plain text"#,
        r#"<img src="data:image/png;base64,Zm9v" width="600">"#,
        r#"</td></tr></table>"#,
        r##"<div style="background:linear-gradient(#fff,#000);">x</div>"##,
        r#"<p style="color:transparent;">x</p>"#,
    );
    let out = darken_colors(html);
    for kept in [
        "padding:4px",
        r#"width="600""#,
        "width:600px",
        "and plain text",
        r#"<img src="data:image/png;base64,Zm9v" width="600">"#,
        "linear-gradient(#fff,#000)",
        "color:transparent",
    ] {
        assert!(out.contains(kept), "{kept} in {out}");
    }
}

#[test]
fn layout_width_takes_the_widest_fixed_width_of_tables_cells_and_blocks() {
    let html = concat!(
        r#"<table width="600"><tr><td width="200">a</td>"#,
        r##"<td style="color:#333;width:640px;">b</td></tr></table>"##,
        r#"<div style="min-width:700px;">c</div>"#,
    );
    assert_eq!(layout_width(html), 700);
    assert_eq!(fit_below(html), 700 + 2 * PAGE_MARGIN_PX);
}

#[test]
fn layout_width_ignores_percentages_and_images() {
    let html = concat!(
        r#"<table width="100%"><tr><td style="width:50%;">"#,
        r#"<img src="data:image/png;base64,Zm9v" width="900"></td></tr></table>"#,
    );
    assert_eq!(layout_width(html), 0);
    assert_eq!(fit_below(html), 0);
}

#[test]
fn fitting_drops_cell_pixel_widths_and_keeps_percentages() {
    assert_eq!(
        fit_widths(r#"<td width="300" style="width:300px;padding:4px;">"#),
        r#"<td style=";padding:4px;">"#
    );
    assert_eq!(fit_widths(r#"<td width="50%">"#), r#"<td width="50%">"#);
}

#[test]
fn fitting_keeps_table_and_block_widths_as_a_cap() {
    assert_eq!(
        fit_widths(r#"<table width="600" align="center">"#),
        r#"<table align="center" style="width:100%;max-width:600px;">"#
    );
    assert_eq!(
        fit_widths(r#"<div style="color:red;width:600px;">"#),
        r#"<div style="color:red;width:100%;max-width:600px;">"#
    );
    // The element's own style still beats the attribute cap.
    assert_eq!(
        fit_widths(r#"<table width="600" style="width:80%;">"#),
        r#"<table style="width:100%;max-width:600px;width:80%;">"#
    );
}

#[test]
fn fitting_drops_pixel_min_widths_and_leaves_images_alone() {
    assert_eq!(
        fit_widths(r#"<div style="min-width:600px;max-width:600px;">"#),
        r#"<div style=";max-width:600px;">"#
    );
    let img = r#"<img src="data:image/png;base64,Zm9v" width="600">"#;
    assert_eq!(fit_widths(img), img);
}

#[test]
fn fitting_does_not_touch_text_or_other_attribute_values() {
    let html = concat!(
        r#"<p>width="600" and width:600px</p>"#,
        r#"<a href="https://example.com/?w=600" rel="noopener">x</a>"#,
    );
    assert_eq!(fit_widths(html), html);
}

#[test]
fn the_document_blocks_every_network_load_by_default() {
    let doc = document("<p>hi</p>", &options(Paint::Theme));
    let policy = csp(&doc);
    assert!(policy.contains("default-src 'none'"), "{policy}");
    assert!(policy.contains("img-src data:;"), "{policy}");
    assert!(!policy.contains("https:"), "{policy}");
    let remote = document(
        "<p>hi</p>",
        &DocumentOptions {
            allow_remote: true,
            ..options(Paint::Theme)
        },
    );
    assert!(csp(&remote).contains("img-src data: https: http:;"));
}

#[test]
fn the_document_styles_blocked_image_disclosures() {
    let doc = document("<p>hi</p>", &options(Paint::Theme));
    assert!(doc.contains(".mc-blocked"), "{doc}");
    assert!(doc.contains(".mc-blocked>summary"), "{doc}");
    assert!(doc.contains("::-webkit-details-marker"), "{doc}");
    assert!(doc.contains(".mc-blocked svg"), "{doc}");
    assert!(doc.contains(".mc-blocked>span"), "{doc}");
}

#[test]
fn the_policy_comes_before_the_body_and_a_spacer_reserves_the_header() {
    let doc = document(
        "<p>marker</p>",
        &DocumentOptions {
            top_space: 121,
            ..options(Paint::Theme)
        },
    );
    assert!(doc.find("Content-Security-Policy").unwrap() < doc.find("marker").unwrap());
    assert!(doc.contains(r#"x-dns-prefetch-control" content="off""#));
    assert!(
        doc.contains(r#"<body><div id="mc-top" style="height:121px"></div><p>marker</p></body>"#)
    );
}

#[test]
fn the_document_loosens_the_layout_only_when_asked() {
    let body = r#"<table width="600"><tr><td width="600">x</td></tr></table>"#;
    let plain = document(body, &options(Paint::Original));
    assert!(plain.contains(body));
    assert!(!plain.contains("div,table{box-sizing:border-box}"));
    assert!(plain.contains("td,th{overflow-wrap:anywhere}"));
    let fitted = document(
        body,
        &DocumentOptions {
            fit: true,
            ..options(Paint::Original)
        },
    );
    assert!(fitted.contains("<td>x</td>"), "{fitted}");
    assert!(fitted.contains("div,table{box-sizing:border-box}"));
}

#[test]
fn a_darkened_document_needs_no_runtime_filter() {
    let doc = document(
        r#"<p style="color:#ffffff;">x</p>"#,
        &options(Paint::Darkened),
    );
    // The page sits straight on the dark surface with the sender's
    // colours pre-inverted: no wrapper, no filter, images untouched.
    assert!(!doc.contains("filter:"));
    assert!(doc.contains("html,body{background:#16181d}"));
    assert!(!doc.contains("color:#ffffff"));
    assert!(doc.contains(&format!("color:{}", LIGHT.ink.inverted().css())));
}

#[test]
fn the_palette_follows_the_paint() {
    let theme = options(Paint::Theme).theme;
    assert_eq!(palette(Paint::Theme, &theme), theme);
    assert_eq!(palette(Paint::Original, &theme), LIGHT);
    let dark = palette(Paint::Darkened, &theme);
    assert_eq!(dark.paper, theme.paper);
    assert_eq!(dark.ink, LIGHT.ink.inverted());
}

#[test]
fn toolkit_css_cannot_close_the_style_element() {
    let doc = document(
        "<p>x</p>",
        &DocumentOptions {
            extra_css: "::-webkit-scrollbar{width:8px}</style><script>",
            ..options(Paint::Theme)
        },
    );
    assert!(!doc.contains("</style><script>"));
    assert!(doc.contains("::-webkit-scrollbar{width:8px}"));
}
