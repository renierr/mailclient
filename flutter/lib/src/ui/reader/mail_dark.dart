import 'package:flutter/material.dart';

import 'mail_fit.dart';
import 'mail_paint.dart';

/// Static dark-mode rewrite for mail HTML: the sender's colours, inverted
/// once at load instead of through a runtime CSS `filter`.
///
/// A `filter: invert(1) hue-rotate(180deg)` over the whole body forces the
/// engine to re-render through the filter on every scrolled frame — a
/// loaded page still filters per frame, which is the stutter dark mails
/// show on phones. Rewriting the colour values up front leaves nothing
/// for the compositor to redo: the page scrolls like any normal document.
///
/// Input is `mailcore::html::sanitize` output (see [fitMailWidths]): there
/// is no `<style>` element left, so every author colour sits in an inline
/// `style` or a `color`/`bgcolor` attribute. Only those change — nothing
/// here can add content, a URL, or anything the sanitizer would not have
/// let through. Values that are not a plain colour (gradients, `url(...)`)
/// are left alone.
///
/// Conversion uses [invertColor], the same matrix as the CSS filter, so a
/// statically darkened mail shows exactly the colours the filter produced.
/// Images need no treatment at all: with no filter running, they simply
/// keep their real colours (previously they were inverted twice).

final _tag = RegExp(r'<([a-z][a-z0-9]*)(\s[^>]*)>');
final _styleAttr = RegExp(r' style="([^"]*)"');
final _colorAttr = RegExp(r' (bgcolor|color)="([^"]*)"');

/// Properties whose value must be one colour to convert.
final _flatDecl = RegExp(
  r'(^|;)\s*(color|background-color|background|border-top-color|'
  r'border-right-color|border-bottom-color|border-left-color|border-color|'
  r'outline-color|text-decoration-color|fill|stroke)\s*:\s*([^;]+)',
  caseSensitive: false,
);

/// `border: 1px solid #ddd` carries width and style next to the colour, so
/// the colour token inside is replaced rather than the whole value.
final _borderDecl = RegExp(
  r'(^|;)\s*(border|border-top|border-right|border-bottom|border-left|outline)'
  r'\s*:\s*([^;]+)',
  caseSensitive: false,
);

/// One colour token inside a shorthand (hex, `rgb()`/`rgba()`, keyword).
final _token = RegExp(r'#[0-9a-fA-F]{3,8}\b|rgba?\([^)]*\)|\b[a-zA-Z]+\b');

final _hex = RegExp(r'^#([0-9a-fA-F]{3}|[0-9a-fA-F]{6}|[0-9a-fA-F]{8})$');
final _rgb = RegExp(
  r'^rgba?\(\s*(\d+(?:\.\d+)?%?)\s*,\s*(\d+(?:\.\d+)?%?)\s*,'
  r'\s*(\d+(?:\.\d+)?%?)\s*(?:,\s*(\d+(?:\.\d+)?%?)\s*)?\)$',
  caseSensitive: false,
);

/// `transparent` and `currentcolor` are not colours to invert.
bool _isKeyword(String word) {
  final w = word.toLowerCase();
  return w != 'transparent' && w != 'currentcolor' && _named.containsKey(w);
}

/// Parse one CSS colour value to a [Color]. Null when the value is not a
/// convertible colour (keywords like `inherit`, gradients, URLs).
Color? _parseColor(String value) {
  final v = value.trim();
  final hex = _hex.firstMatch(v);
  if (hex != null) {
    var digits = hex[1]!;
    if (digits.length == 3) {
      digits = digits.split('').map((c) => '$c$c').join();
    }
    var alpha = 255;
    if (digits.length == 8) {
      alpha = int.parse(digits.substring(6), radix: 16);
      digits = digits.substring(0, 6);
    }
    final rgb = int.parse(digits, radix: 16);
    return Color.fromARGB(
      alpha,
      (rgb >> 16) & 0xFF,
      (rgb >> 8) & 0xFF,
      rgb & 0xFF,
    );
  }
  final rgb = _rgb.firstMatch(v);
  if (rgb != null) {
    int channel(String s) => s.endsWith('%')
        ? (double.parse(s.substring(0, s.length - 1)) * 255 / 100).round()
        : int.parse(s);
    var alpha = 255;
    if (rgb[4] != null) {
      final a = rgb[4]!;
      alpha = a.endsWith('%')
          ? (double.parse(a.substring(0, a.length - 1)) * 255 / 100).round()
          : (double.parse(a) * 255).round();
    }
    return Color.fromARGB(
      alpha.clamp(0, 255),
      channel(rgb[1]!).clamp(0, 255),
      channel(rgb[2]!).clamp(0, 255),
      channel(rgb[3]!).clamp(0, 255),
    );
  }
  if (_isKeyword(v)) return _colorFromHex(_named[v.toLowerCase()]!);
  return null;
}

Color _colorFromHex(String hex) {
  final rgb = int.parse(hex.substring(1), radix: 16);
  return Color.fromARGB(255, (rgb >> 16) & 0xFF, (rgb >> 8) & 0xFF, rgb & 0xFF);
}

/// A [Color] back to CSS: opaque as `#rrggbb`, translucent as `rgba(...)`.
String _toCss(Color c) {
  final a = (c.a * 255).round();
  if (a >= 255) return cssHex(c);
  var alpha = (a / 255).toStringAsFixed(3).replaceAll(RegExp(r'0+$'), '');
  if (alpha.endsWith('.')) alpha = '${alpha}0';
  return 'rgba(${(c.r * 255).round()},${(c.g * 255).round()},'
      '${(c.b * 255).round()},$alpha)';
}

/// Convert one colour value, keeping a trailing `!important`. Returns the
/// input unchanged when it holds no convertible colour.
String _convertValue(String value) {
  var v = value;
  var important = '';
  final bang = RegExp(r'\s*!important\s*$', caseSensitive: false);
  final bangAt = bang.firstMatch(v);
  if (bangAt != null) {
    important = bangAt[0]!;
    v = v.substring(0, bangAt.start);
  }
  final color = _parseColor(v);
  if (color == null) return value;
  return '${_toCss(invertColor(color))}$important';
}

/// [html] with the sender's colours inverted for a dark page. See above.
String darkenMailColors(String html) => html.replaceAllMapped(_tag, (m) {
  var attrs = m[2]!;
  attrs = attrs.replaceFirstMapped(_styleAttr, (s) {
    var style = s[1]!;
    style = style.replaceAllMapped(_flatDecl, (d) {
      final converted = _convertValue(d[3]!);
      if (converted == d[3]) return d[0]!;
      return '${d[1]}${d[2]}:$converted';
    });
    style = style.replaceAllMapped(_borderDecl, (d) {
      // Shorthands with images or gradients keep the author's value.
      if (d[3]!.contains('url(') || d[3]!.contains('gradient')) {
        return d[0]!;
      }
      final converted = d[3]!.replaceAllMapped(_token, (t) {
        final word = t[0]!;
        final lower = word.toLowerCase();
        final isColor =
            word.startsWith('#') ||
            word.toLowerCase().startsWith('rgb') ||
            _isKeyword(word);
        if (!isColor || lower == 'transparent' || lower == 'currentcolor') {
          return word;
        }
        return _convertValue(word);
      });
      if (converted == d[3]) return d[0]!;
      return '${d[1]}${d[2]}:$converted';
    });
    return ' style="$style"';
  });
  attrs = attrs.replaceFirstMapped(_colorAttr, (c) {
    final converted = _convertValue(c[2]!);
    if (converted == c[2]) return c[0]!;
    return ' ${c[1]}="$converted"';
  });
  return '<${m[1]}$attrs>';
});

/// The CSS colour keywords and their hex values.
const _named = <String, String>{
  'aliceblue': '#f0f8ff',
  'antiquewhite': '#faebd7',
  'aqua': '#00ffff',
  'aquamarine': '#7fffd4',
  'azure': '#f0ffff',
  'beige': '#f5f5dc',
  'bisque': '#ffe4c4',
  'black': '#000000',
  'blanchedalmond': '#ffebcd',
  'blue': '#0000ff',
  'blueviolet': '#8a2be2',
  'brown': '#a52a2a',
  'burlywood': '#deb887',
  'cadetblue': '#5f9ea0',
  'chartreuse': '#7fff00',
  'chocolate': '#d2691e',
  'coral': '#ff7f50',
  'cornflowerblue': '#6495ed',
  'cornsilk': '#fff8dc',
  'crimson': '#dc143c',
  'cyan': '#00ffff',
  'darkblue': '#00008b',
  'darkcyan': '#008b8b',
  'darkgoldenrod': '#b8860b',
  'darkgray': '#a9a9a9',
  'darkgrey': '#a9a9a9',
  'darkgreen': '#006400',
  'darkkhaki': '#bdb76b',
  'darkmagenta': '#8b008b',
  'darkolivegreen': '#556b2f',
  'darkorange': '#ff8c00',
  'darkorchid': '#9932cc',
  'darkred': '#8b0000',
  'darksalmon': '#e9967a',
  'darkseagreen': '#8fbc8f',
  'darkslateblue': '#483d8b',
  'darkslategray': '#2f4f4f',
  'darkslategrey': '#2f4f4f',
  'darkturquoise': '#00ced1',
  'darkviolet': '#9400d3',
  'deeppink': '#ff1493',
  'deepskyblue': '#00bfff',
  'dimgray': '#696969',
  'dimgrey': '#696969',
  'dodgerblue': '#1e90ff',
  'firebrick': '#b22222',
  'floralwhite': '#fffaf0',
  'forestgreen': '#228b22',
  'fuchsia': '#ff00ff',
  'gainsboro': '#dcdcdc',
  'ghostwhite': '#f8f8ff',
  'gold': '#ffd700',
  'goldenrod': '#daa520',
  'gray': '#808080',
  'grey': '#808080',
  'green': '#008000',
  'greenyellow': '#adff2f',
  'honeydew': '#f0fff0',
  'hotpink': '#ff69b4',
  'indianred': '#cd5c5c',
  'indigo': '#4b0082',
  'ivory': '#fffff0',
  'khaki': '#f0e68c',
  'lavender': '#e6e6fa',
  'lavenderblush': '#fff0f5',
  'lawngreen': '#7cfc00',
  'lemonchiffon': '#fffacd',
  'lightblue': '#add8e6',
  'lightcoral': '#f08080',
  'lightcyan': '#e0ffff',
  'lightgoldenrodyellow': '#fafad2',
  'lightgray': '#d3d3d3',
  'lightgrey': '#d3d3d3',
  'lightgreen': '#90ee90',
  'lightpink': '#ffb6c1',
  'lightsalmon': '#ffa07a',
  'lightseagreen': '#20b2aa',
  'lightskyblue': '#87cefa',
  'lightslategray': '#778899',
  'lightslategrey': '#778899',
  'lightsteelblue': '#b0c4de',
  'lightyellow': '#ffffe0',
  'lime': '#00ff00',
  'limegreen': '#32cd32',
  'linen': '#faf0e6',
  'magenta': '#ff00ff',
  'maroon': '#800000',
  'mediumaquamarine': '#66cdaa',
  'mediumblue': '#0000cd',
  'mediumorchid': '#ba55d3',
  'mediumpurple': '#9370db',
  'mediumseagreen': '#3cb371',
  'mediumslateblue': '#7b68ee',
  'mediumspringgreen': '#00fa9a',
  'mediumturquoise': '#48d1cc',
  'mediumvioletred': '#c71585',
  'midnightblue': '#191970',
  'mintcream': '#f5fffa',
  'mistyrose': '#ffe4e1',
  'moccasin': '#ffe4b5',
  'navajowhite': '#ffdead',
  'navy': '#000080',
  'oldlace': '#fdf5e6',
  'olive': '#808000',
  'olivedrab': '#6b8e23',
  'orange': '#ffa500',
  'orangered': '#ff4500',
  'orchid': '#da70d6',
  'palegoldenrod': '#eee8aa',
  'palegreen': '#98fb98',
  'paleturquoise': '#afeeee',
  'palevioletred': '#db7093',
  'papayawhip': '#ffefd5',
  'peachpuff': '#ffdab9',
  'peru': '#cd853f',
  'pink': '#ffc0cb',
  'plum': '#dda0dd',
  'powderblue': '#b0e0e6',
  'purple': '#800080',
  'rebeccapurple': '#663399',
  'red': '#ff0000',
  'rosybrown': '#bc8f8f',
  'royalblue': '#4169e1',
  'saddlebrown': '#8b4513',
  'salmon': '#fa8072',
  'sandybrown': '#f4a460',
  'seagreen': '#2e8b57',
  'seashell': '#fff5ee',
  'sienna': '#a0522d',
  'silver': '#c0c0c0',
  'skyblue': '#87ceeb',
  'slateblue': '#6a5acd',
  'slategray': '#708090',
  'slategrey': '#708090',
  'snow': '#fffafa',
  'springgreen': '#00ff7f',
  'steelblue': '#4682b4',
  'tan': '#d2b48c',
  'teal': '#008080',
  'thistle': '#d8bfd8',
  'tomato': '#ff6347',
  'turquoise': '#40e0d0',
  'violet': '#ee82ee',
  'wheat': '#f5deb3',
  'white': '#ffffff',
  'whitesmoke': '#f5f5f5',
  'yellow': '#ffff00',
  'yellowgreen': '#9acd32',
};
