# Bundled icon font

`MaterialIcons-Regular.ttf` — Google's Material Icons (filled), Apache
License 2.0, https://github.com/google/material-design-icons

It is bundled (rather than a system-font dependency) so the app renders
identically with or without any particular desktop fonts, on Linux and
Windows alike. The codepoint for each used icon lives in `../Icons.qml`;
the mapping was taken from the upstream `font/codepoints` file and every
entry was verified against this exact TTF before use.

License note: a copy of the Apache 2.0 text ships with upstream at
https://github.com/google/material-design-icons/blob/master/LICENSE
