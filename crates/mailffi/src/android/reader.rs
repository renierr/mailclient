//! The native reader: payloads, document building and view helpers.

use super::*;

use jni::errors::ThrowRuntimeExAndDefault;
use jni::objects::{JClass, JString};
use jni::EnvUnowned;

/// Experiment: native reader (`ReaderActivity.kt`, branch
/// `experiment/native-reader`). The activity passes ids only (never HTML —
/// bodies with inline images exceed the Binder limit) and re-reads from the
/// same database Dart uses.
///
/// `MailNative.readerMessage(folderId, uid)`: full reader payload, same JSON
/// as `api::messages::message_json` (sanitized bodies included).
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_readerMessage<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    folder_id: i64,
    uid: i32,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            let json =
                mailcore::feed::message_json(crate::db::shared_db()?, folder_id, uid_arg(uid)?)?;
            Ok(env.new_string(json)?)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.readerHeaders(folderId, uid)`: `{from, to, cc, date, subject,
/// message_id, reply_to}`, same JSON as `api::messages::headers_json`.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_readerHeaders<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    folder_id: i64,
    uid: i32,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            let json =
                mailcore::feed::headers_json(crate::db::shared_db()?, folder_id, uid_arg(uid)?)?;
            Ok(env.new_string(json)?)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.readerDocument(body, paint, paper, ink, link, quote, rule,
/// allowRemote, scale, fit)`: full document for a `WebView`, same builder as
/// `api::reader::reader_document` but with `top_space: 0` — the native
/// activity lays the header out as views above the `WebView` in one scroll,
/// so there is no overlay spacer.
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_readerDocument<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    body: JString<'caller>,
    paint: JString<'caller>,
    paper: i32,
    ink: i32,
    link: i32,
    quote: i32,
    rule: i32,
    allow_remote: bool,
    scale: f32,
    fit: bool,
) -> JString<'caller> {
    use mailcore::html::reader::{self, Palette, Rgb};
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            let rgb = |v: i32| Rgb(v as u32 & 0xFF_FFFF);
            let doc = reader::document(
                &string(env, &body)?,
                &reader::DocumentOptions {
                    paint: reader::Paint::parse(&string(env, &paint)?),
                    theme: Palette {
                        paper: rgb(paper),
                        ink: rgb(ink),
                        link: rgb(link),
                        quote: rgb(quote),
                        rule: rgb(rule),
                    },
                    allow_remote,
                    top_space: 0,
                    scale,
                    fit,
                    extra_css: "",
                },
            );
            Ok(env.new_string(doc)?)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

// Reader actions. Same rule as everywhere else: translation only — every
// one of these mirrors a `mailffi::api` function over the same database and
// net thread Dart uses, so both readers queue the same jobs.

/// `MailNative.readerMessageHtml(folderId, uid, allowRemote)`: the "show
/// remote images once" path — `feed::message_html` re-sanitized with remote
/// references kept.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_readerMessageHtml<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    folder_id: i64,
    uid: i32,
    allow_remote: bool,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            let html = mailcore::feed::message_html(
                crate::db::shared_db()?,
                folder_id,
                uid_arg(uid)?,
                allow_remote,
            )?;
            Ok(env.new_string(html)?)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.readerTextScale(size)`: the reader text size's factor
/// (`mailcore::store::settings::reader_text_scale`).
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_readerTextScale<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    size: JString<'caller>,
) -> f32 {
    unowned
        .with_env(|env| -> Result<f32> {
            Ok(mailcore::store::settings::reader_text_scale(&string(
                env, &size,
            )?))
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.linkInfo(url)`: a clicked link split for the examine dialog,
/// and whether it may be opened at all — `{"safe","scheme","host","path"}`.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_linkInfo<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    url: JString<'caller>,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            let i = mailcore::html::link_info(&string(env, &url)?);
            let json = serde_json::json!({
                "safe": i.safe,
                "scheme": i.scheme,
                "host": i.host,
                "path": i.path,
            });
            Ok(env.new_string(serde_json::to_string(&json)?)?)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

fn reader_paint_name(p: crate::api::reader::ReaderPaint) -> &'static str {
    match p {
        crate::api::reader::ReaderPaint::Theme => "theme",
        crate::api::reader::ReaderPaint::Original => "original",
        crate::api::reader::ReaderPaint::Darkened => "darkened",
    }
}

fn parse_paint(raw: &str) -> Result<crate::api::reader::ReaderPaint> {
    use crate::api::reader::ReaderPaint;
    match raw {
        "theme" => Ok(ReaderPaint::Theme),
        "original" => Ok(ReaderPaint::Original),
        "darkened" => Ok(ReaderPaint::Darkened),
        _ => Err(BridgeError(format!("bad paint: {raw}"))),
    }
}

/// `MailNative.readerPaint(colored, dark, keepOriginal)`: the paint for one
/// mail — `"theme"`, `"original"` or `"darkened"`.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_readerPaint<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    colored: bool,
    dark: bool,
    keep_original: bool,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            Ok(
                env.new_string(reader_paint_name(crate::api::reader::reader_paint(
                    colored,
                    dark,
                    keep_original,
                )))?,
            )
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.readerPalette(paint, paper, ink, link, quote, rule)`: what
/// `paint` writes the page in — `{"paper","ink","link","quote","rule"}` as
/// `0xRRGGBB` ints.
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_readerPalette<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    paint: JString<'caller>,
    paper: i32,
    ink: i32,
    link: i32,
    quote: i32,
    rule: i32,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            let rgb = |v: i32| (v as u32) & 0xFF_FFFF;
            let pal = crate::api::reader::reader_palette(
                parse_paint(&string(env, &paint)?)?,
                crate::api::reader::ReaderPalette {
                    paper: rgb(paper),
                    ink: rgb(ink),
                    link: rgb(link),
                    quote: rgb(quote),
                    rule: rgb(rule),
                },
            );
            let json = serde_json::json!({
                "paper": pal.paper,
                "ink": pal.ink,
                "link": pal.link,
                "quote": pal.quote,
                "rule": pal.rule,
            });
            Ok(env.new_string(serde_json::to_string(&json)?)?)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.readerFitBelow(body)`: pages narrower than this get fixed
/// widths loosened — back as a string, `0` when the mail has none.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_readerFitBelow<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    body: JString<'caller>,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            Ok(env.new_string(
                crate::api::reader::reader_fit_below(string(env, &body)?).to_string(),
            )?)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.canToggleColors(isHtml, htmlColored, dark, fitBelow)`:
/// whether the reader offers its colours/layout toggle
/// (`mailcore::html::reader::can_toggle_colors`).
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_canToggleColors<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    is_html: jni::sys::jboolean,
    html_colored: jni::sys::jboolean,
    dark: jni::sys::jboolean,
    fit_below: i32,
) -> bool {
    unowned
        .with_env(|_env| -> Result<bool> {
            Ok(mailcore::html::reader::can_toggle_colors(
                is_html,
                html_colored,
                dark,
                u32::try_from(fit_below.max(0)).unwrap_or(0),
            ))
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.readerDocumentFull(body, paint, paper, ink, link, quote,
/// rule, allowRemote, topSpace, scale, fit)`: the full document with every
/// option — unlike `readerDocument`, which fixes `top_space: 0` for the
/// Views experiment layout.
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_readerDocumentFull<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    body: JString<'caller>,
    paint: JString<'caller>,
    paper: i32,
    ink: i32,
    link: i32,
    quote: i32,
    rule: i32,
    allow_remote: bool,
    top_space: i32,
    scale: f32,
    fit: bool,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            let rgb = |v: i32| (v as u32) & 0xFF_FFFF;
            Ok(env.new_string(crate::api::reader::reader_document(
                string(env, &body)?,
                crate::api::reader::ReaderDocumentOptions {
                    paint: parse_paint(&string(env, &paint)?)?,
                    theme: crate::api::reader::ReaderPalette {
                        paper: rgb(paper),
                        ink: rgb(ink),
                        link: rgb(link),
                        quote: rgb(quote),
                        rule: rgb(rule),
                    },
                    allow_remote,
                    top_space: u32::try_from(top_space.max(0)).unwrap_or(u32::MAX),
                    scale: f64::from(scale),
                    fit,
                },
            ))?)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}
