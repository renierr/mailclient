import 'dart:math' as math;

/// Fixed-width newsletter layouts on a screen narrower than their design.
///
/// Designed mail is laid out at 600–700px and says so with pixel `width`
/// attributes and inline styles. The document CSS caps tables and images at
/// the page width, but in table layout a cell's pixel width is a *minimum*,
/// so a row of fixed cells (or a `min-width`) still pushes the page sideways.
/// When the layout is wider than the page, [fitMailWidths] turns those fixed
/// widths into caps so it can shrink; where the mail fits, it stays exactly
/// as designed.
///
/// Input is `mailcore::html::sanitize` output: lowercase tags, attributes as
/// ` name="value"` with quotes escaped inside values, `width` attributes as
/// bare pixels or a percentage. Only widths change — nothing here can add
/// content, a URL, or anything the sanitizer would not have let through.

final _tag = RegExp(r'<([a-z][a-z0-9]*)(\s[^>]*)>');
final _widthAttr = RegExp(r' width="(\d+)"');
final _styleAttr = RegExp(r' style="([^"]*)"');
final _pxWidth = RegExp(
  r'(^|;)\s*(min-width|width)\s*:\s*(\d+(?:\.\d+)?)\s*px\s*(?=;|$)',
  caseSensitive: false,
);

/// Cells take their width from the table once their own is gone.
bool _isCell(String tag) => tag == 'td' || tag == 'th' || tag == 'col';

/// Widest fixed pixel width the layout asks for: tables, cells, blocks.
/// Images are left out — the document CSS already shrinks them. 0 for mail
/// with no fixed widths.
int mailLayoutWidth(String html) {
  var widest = 0;
  for (final m in _tag.allMatches(html)) {
    if (m[1] == 'img') continue;
    final attrs = m[2]!;
    final attr = _widthAttr.firstMatch(attrs);
    if (attr != null) widest = math.max(widest, int.parse(attr[1]!));
    final style = _styleAttr.firstMatch(attrs);
    if (style == null) continue;
    for (final d in _pxWidth.allMatches(style[1]!)) {
      widest = math.max(widest, double.parse(d[3]!).ceil());
    }
  }
  return widest;
}

/// [html] with fixed widths loosened so the layout fits a narrow page:
/// cells lose their pixel widths (the table shares out what is there),
/// other elements keep theirs as a `max-width` and otherwise fill their
/// container, and pixel `min-width`s go.
String fitMailWidths(String html) => html.replaceAllMapped(_tag, (m) {
  final tag = m[1]!;
  if (tag == 'img') return m[0]!;
  final cell = _isCell(tag);
  var attrs = m[2]!;
  String? cap;
  attrs = attrs.replaceFirstMapped(_widthAttr, (w) {
    if (!cell) cap = 'width:100%;max-width:${w[1]}px;';
    return '';
  });
  var styled = false;
  attrs = attrs.replaceFirstMapped(_styleAttr, (s) {
    styled = true;
    final kept = s[1]!.replaceAllMapped(_pxWidth, (d) {
      final lead = d[1]!;
      if (cell || d[2]!.toLowerCase() == 'min-width') return lead;
      return '${lead}width:100%;max-width:${d[3]}px';
    });
    // The element's own style still wins over the attribute's cap.
    return ' style="${cap ?? ''}$kept"';
  });
  if (!styled && cap != null) attrs = '$attrs style="$cap"';
  return '<$tag$attrs>';
});
