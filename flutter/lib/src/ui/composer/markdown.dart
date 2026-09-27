/// Minimal Markdown → HTML for the composer body.
///
/// The editor is plain text with Markdown syntax (see the toolbar); this
/// renders the subset the toolbar produces — bold, italic, quotes, bullets,
/// links — to HTML so the formatting actually arrives instead of going out
/// as literal asterisks. Only [toHtml] output for text where [hasFormatting]
/// is true ever reaches the core's `body_html`, so plain mail keeps going
/// out as plain text exactly like before.
///
/// The core sanitizes `body_html` again on send (`sanitize_for_send`), so
/// this only builds structure — escaping here is defense in depth, not the
/// security boundary.
abstract final class MarkdownMail {
  static final _bold = RegExp(r'\*\*([^*]+)\*\*');
  static final _starPair = RegExp(r'\*([^*]+)\*');
  static final _link = RegExp(r'\[([^\]]+)\]\(((?:[^()]*|\([^()]*\))*)\)');
  static final _quoteLine = RegExp(r'^> ?');
  static final _bulletLine = RegExp(r'^[-*] ');
  static final _wordChar = RegExp(r'[\w*]');

  /// Whether the text carries any formatting this converter renders. The
  /// composer only sends `body_html` when this is true.
  static bool hasFormatting(String text) {
    if (_bold.hasMatch(text) || _hasItalic(text)) return true;
    for (final line in text.split('\n')) {
      final t = line.trimLeft();
      if (t.startsWith('>') || _bulletLine.hasMatch(t)) return true;
      if (_link.hasMatch(t)) return true;
    }
    return false;
  }

  /// A `*…*` pair counts as italic only with non-word boundaries on both
  /// sides — otherwise `2*3=6 and a * b` (stars glued to math/words) would
  /// falsely mark plain prose as formatted. The `**bold**` inner pair is
  /// excluded the same way (its "opener" is glued to `*`).
  static bool _hasItalic(String text) {
    for (final m in _starPair.allMatches(text)) {
      final before = m.start == 0 ? '' : text[m.start - 1];
      final after = m.end >= text.length ? '' : text[m.end];
      if ((before.isEmpty || !_wordChar.hasMatch(before)) &&
          (after.isEmpty || !_wordChar.hasMatch(after))) {
        return true;
      }
    }
    return false;
  }

  /// Render the supported subset to an HTML fragment (no `<html>` wrapper —
  /// the core sanitizes and envelopes it).
  static String toHtml(String text) {
    final out = StringBuffer();
    List<String>? quote;
    List<String>? bullets;

    void flushQuote() {
      if (quote == null) return;
      out.write('<blockquote>${quote!.map(_inline).join('<br>')}</blockquote>');
      quote = null;
    }

    void flushBullets() {
      if (bullets == null) return;
      out.write(
        '<ul>${bullets!.map((l) => '<li>${_inline(l)}</li>').join()}</ul>',
      );
      bullets = null;
    }

    for (final raw in text.split('\n')) {
      final line = raw.trimRight();
      if (line.trimLeft().startsWith('>')) {
        flushBullets();
        quote ??= [];
        quote!.add(line.replaceFirst(_quoteLine, ''));
      } else if (_bulletLine.hasMatch(line.trimLeft())) {
        flushQuote();
        bullets ??= [];
        bullets!.add(line.trimLeft().substring(2));
      } else if (line.trim().isEmpty) {
        flushQuote();
        flushBullets();
      } else {
        flushQuote();
        flushBullets();
        out.write('<p>${_inline(line)}</p>');
      }
    }
    flushQuote();
    flushBullets();
    return out.toString();
  }

  /// Inline spans after HTML-escaping: links (web schemes only — anything
  /// else renders as its visible text), then bold, then italic.
  static String _inline(String text) {
    var s = text
        .replaceAll('&', '&amp;')
        .replaceAll('<', '&lt;')
        .replaceAll('>', '&gt;');
    s = s.replaceAllMapped(_link, (m) {
      final url = m.group(2)!;
      final lower = url.trim().toLowerCase();
      if (lower.startsWith('http://') ||
          lower.startsWith('https://') ||
          lower.startsWith('mailto:')) {
        return '<a href="$url">${m.group(1)!}</a>';
      }
      return m.group(1)!;
    });
    s = s.replaceAllMapped(_bold, (m) => '<b>${m.group(1)!}</b>');
    // Same boundary rule as detection: only real pairs render.
    s = _renderItalic(s);
    return s;
  }

  static String _renderItalic(String s) {
    final out = StringBuffer();
    var pos = 0;
    for (final m in _starPair.allMatches(s)) {
      final before = m.start == 0 ? '' : s[m.start - 1];
      final after = m.end >= s.length ? '' : s[m.end];
      // Bold pairs were already replaced above; whatever `*…*` remains with
      // word-glued ends is literal text, left untouched.
      if ((before.isEmpty || !_wordChar.hasMatch(before)) &&
          (after.isEmpty || !_wordChar.hasMatch(after))) {
        out.write(s.substring(pos, m.start));
        out.write('<i>${m.group(1)!}</i>');
        pos = m.end;
      }
    }
    out.write(s.substring(pos));
    return out.toString();
  }
}
