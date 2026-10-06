//! The WYSIWYG body editor's document, for toolkits that edit HTML in a web
//! view (native Android; Qt's `EditorFrame.qml` still builds its own, see
//! SHARED-CORE.md).
//!
//! The document is ours, not mail, so it runs a small script: a
//! `contentEditable` body driven by `document.execCommand` with
//! `styleWithCSS` off, which emits real `<b>`/`<i>`/`<u>` tags — the ones the
//! outgoing sanitizer keeps (inline `style` is stripped). The body it starts
//! with is already sanitized (answer quotes, stored drafts) or typed by the
//! user; a CSP with a per-document nonce keeps any script or `on*` handler
//! inside it from running, and nothing is fetched beyond `data:` images.
//!
//! Bold, italic and underline at a bare caret do not use `execCommand`: it
//! only arms a pending "typing style", which an Android keyboard drops as
//! soon as it replaces the word being composed (auto-capitalisation,
//! autocorrect). Instead a real `<b>`/`<i>`/`<u>` holding a zero-width space
//! goes in at the caret (or the run is split to step out of it), so the
//! keyboard types inside an element. `mc.html()` strips those spaces and the
//! formatting tags left empty, so an unused toggle sends nothing.
//!
//! The toolkit drives the page through `window.mc` (`exec`, `quote`, `link`,
//! `save`, `html`, `top`, `state`, `focus`). A host that can receive calls
//! from the page exposes `window.MCHost` with `changed()` (the user edited)
//! and `state(json)` (formatting at the caret changed: `b`, `i`, `u`, `l`
//! list, `q` quote); one that cannot polls `mc.state()` instead.

use crate::html::{escape_attr, needs_html_formatting, sanitize_for_send};

/// The editor's colours (`0xRRGGBB`) and text size, from the app theme.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EditorStyle {
    pub paper: u32,
    pub ink: u32,
    pub muted: u32,
    pub accent: u32,
    pub rule: u32,
    /// Body text size in CSS px (the toolkit scales it further, if at all).
    pub font_px: u32,
}

fn css(rgb: u32) -> String {
    format!("#{:06x}", rgb & 0xFF_FFFF)
}

/// The whole editor page with `body_html` as the starting text. The editable
/// region is `#e`; `#mc-top` above it is a spacer the toolkit sizes
/// (`mc.top(px)`) when its own header overlays the top of the page, the
/// reader's layout.
#[must_use]
pub fn document(style: &EditorStyle, placeholder: &str, body_html: &str) -> String {
    let nonce = uuid::Uuid::new_v4().simple().to_string();
    let csp = format!(
        "default-src 'none'; img-src data:; style-src 'unsafe-inline'; \
         script-src 'nonce-{nonce}'; font-src 'none'; media-src 'none'; frame-src 'none'; \
         form-action 'none'; base-uri 'none'"
    );
    let (paper, ink, muted, accent, rule) = (
        css(style.paper),
        css(style.ink),
        css(style.muted),
        css(style.accent),
        css(style.rule),
    );
    let font = style.font_px.clamp(8, 48);
    let placeholder = escape_attr(placeholder);
    format!(
        "<!DOCTYPE html><html><head><meta charset=\"utf-8\">\
         <meta http-equiv=\"Content-Security-Policy\" content=\"{csp}\">\
         <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\
         <style>html,body{{margin:0;padding:0;background:{paper}}}\
         #e{{box-sizing:border-box;min-height:100vh;padding:12px 16px 48px;outline:none;\
         font-family:sans-serif;font-size:{font}px;line-height:1.55;color:{ink};\
         caret-color:{accent};overflow-wrap:break-word}}\
         #e:empty:before{{content:attr(data-placeholder);color:{muted}}}\
         #e p{{margin:0 0 .6em}}#e p:empty{{min-height:1.55em}}\
         img{{max-width:100%;height:auto}}\
         blockquote{{margin:8px 0;padding-left:12px;border-left:3px solid {rule};color:{muted}}}\
         a{{color:{accent}}}</style></head>\
         <body><div id=\"mc-top\"></div><div id=\"e\" contenteditable=\"true\" data-placeholder=\"{placeholder}\">{body_html}</div>\
         <script nonce=\"{nonce}\">{SCRIPT}</script></body></html>"
    )
}

/// What Send will produce for the send-format setting and the editor's
/// current HTML, in words. `auto` follows the sender's own rule
/// (`sync::sender`): plain text unless the body carries real formatting.
#[must_use]
pub fn send_format_note(format: &str, html: &str) -> &'static str {
    match crate::store::settings::normalize_send_format(format) {
        "plain" => "Sends as plain text",
        "html" => "Sends as HTML",
        "multipart" => "Sends as plain text and HTML",
        _ if needs_html_formatting(&sanitize_for_send(html)) => "Sends formatted (HTML)",
        _ => "Sends as plain text",
    }
}

/// Kept on one line per statement: it is embedded verbatim.
const SCRIPT: &str = r#"(function(){
var e=document.getElementById('e'),spacer=document.getElementById('mc-top');
document.execCommand('defaultParagraphSeparator',false,'p');
document.execCommand('styleWithCSS',false,false);
var host=window.MCHost||null,saved=null,last='';
function sel(){var s=getSelection();return s.rangeCount?s.getRangeAt(0):null;}
function inside(r){return !!r&&e.contains(r.commonAncestorContainer);}
function restore(){if(!saved)return;var s=getSelection();s.removeAllRanges();s.addRange(saved);saved=null;}
function quoted(){return /blockquote/i.test(document.queryCommandValue('formatBlock'));}
function state(){return JSON.stringify({b:document.queryCommandState('bold'),i:document.queryCommandState('italic'),u:document.queryCommandState('underline'),l:document.queryCommandState('insertUnorderedList'),q:quoted()});}
function report(){if(!host)return;var s=state();if(s!==last){last=s;host.state(s);}}
function edited(){if(host)host.changed();report();}
var TAGS={bold:['B','STRONG'],italic:['I','EM'],underline:['U']};
function holder(n,names){for(;n&&n!==e;n=n.parentNode){if(n.nodeType===1&&names.indexOf(n.nodeName)>=0)return n;}return null;}
function place(n,o){var r=document.createRange();r.setStart(n,o);r.collapse(true);var s=getSelection();s.removeAllRanges();s.addRange(r);}
function toggle(c){var r=sel(),names=TAGS[c];
if(!inside(r)||!r.collapsed){document.execCommand(c,false,null);return;}
var z=document.createTextNode('\u200B'),h=holder(r.startContainer,names);
if(h){var t=document.createRange();t.setStart(r.startContainer,r.startOffset);t.setEndAfter(h);var rest=t.extractContents();
h.parentNode.insertBefore(z,h.nextSibling);
if(rest.textContent.replace(/\u200B/g,'')!==''||rest.querySelector&&rest.querySelector('img'))z.parentNode.insertBefore(rest,z.nextSibling);}
else{var el=document.createElement(names[0]);el.appendChild(z);r.insertNode(el);}
place(z,1);}
function clean(root){var w=document.createTreeWalker(root,NodeFilter.SHOW_TEXT),t;
while((t=w.nextNode())){if(t.data.indexOf('\u200B')>=0)t.data=t.data.replace(/\u200B/g,'');}
root.querySelectorAll('b,strong,i,em,u').forEach(function(n){if(n.textContent===''&&!n.querySelector('img')&&n.parentNode)n.parentNode.removeChild(n);});}
e.addEventListener('input',edited);
document.addEventListener('selectionchange',report);
window.mc={
exec:function(c,v){e.focus();restore();if(TAGS[c])toggle(c);else document.execCommand(c,false,v===undefined?null:v);edited();},
quote:function(){mc.exec('formatBlock',quoted()?'p':'blockquote');},
link:function(u){e.focus();restore();var r=sel();if(!inside(r))return;
if(r.collapsed){var a=document.createElement('a');a.href=u;a.textContent=u;r.insertNode(a);r.setStartAfter(a);r.collapse(true);var s=getSelection();s.removeAllRanges();s.addRange(r);}
else{document.execCommand('createLink',false,u);}
edited();},
save:function(){var r=sel();saved=inside(r)?r.cloneRange():null;},
html:function(){var c=e.cloneNode(true);clean(c);return c.innerHTML;},
top:function(px){spacer.style.height=px+'px';},
state:state,
focus:function(){e.focus();}
};
})();"#;

#[cfg(test)]
mod tests {
    use super::*;

    fn style() -> EditorStyle {
        EditorStyle {
            paper: 0x00_11_22,
            ink: 0xFF_FF_FF,
            muted: 0x80_80_80,
            accent: 0x3B_82_F6,
            rule: 0x44_44_44,
            font_px: 16,
        }
    }

    #[test]
    fn document_carries_body_colours_and_a_nonced_script() {
        let doc = document(&style(), "Write your message", "<p>Hi</p>");
        assert!(doc.contains("data-placeholder=\"Write your message\"><p>Hi</p></div>"));
        assert!(doc.contains("background:#001122"));
        assert!(doc.contains("caret-color:#3b82f6"));
        let nonce = doc
            .split("'nonce-")
            .nth(1)
            .and_then(|s| s.split('\'').next())
            .unwrap();
        assert_eq!(nonce.len(), 32);
        assert!(doc.contains(&format!("<script nonce=\"{nonce}\">")));
        // Only data: images load; nothing else is fetched.
        assert!(doc.contains("default-src 'none'; img-src data:;"));
    }

    #[test]
    fn every_document_gets_its_own_nonce() {
        let a = document(&style(), "", "");
        let b = document(&style(), "", "");
        assert_ne!(a, b);
    }

    #[test]
    fn placeholder_cannot_break_out_of_its_attribute() {
        let doc = document(&style(), "a\"><script>x()</script>", "");
        assert!(!doc.contains("<script>x()"));
        assert!(doc.contains("data-placeholder=\"a&quot;&gt;&lt;script&gt;"));
    }

    #[test]
    fn note_follows_the_sender_rule_for_auto() {
        assert_eq!(
            send_format_note("auto", "<p>hello</p>"),
            "Sends as plain text"
        );
        assert_eq!(
            send_format_note("", "<p>hello<br></p>"),
            "Sends as plain text"
        );
        assert_eq!(
            send_format_note("auto", "<p><b>hello</b></p>"),
            "Sends formatted (HTML)"
        );
        assert_eq!(
            send_format_note("auto", "<blockquote>q</blockquote>"),
            "Sends formatted (HTML)"
        );
        assert_eq!(send_format_note("plain", "<b>x</b>"), "Sends as plain text");
        assert_eq!(send_format_note("html", "x"), "Sends as HTML");
        assert_eq!(
            send_format_note("multipart", "x"),
            "Sends as plain text and HTML"
        );
    }
}
