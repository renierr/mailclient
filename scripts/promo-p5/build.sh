#!/usr/bin/env bash
# mailclient promo v2 — deterministic motion-graphics render.
# Design: sketch.js (p5.js timeline, frame-accurate, no wall-clock).
# Renderer: render.py (ImageMagick, native Noto Sans, static navy bg).
#   (Browser capture is blocked here: Chromium can't resolve any font family,
#    and Qt offscreen windows never produce frames — see render.py header.)
# Music: music.py (stdlib synth, Am F C G pad + bass + arp), loudness-verified.
# Usage: ./scripts/promo-p5/build.sh  -> dist/promo/mailclient-promo-v2.mp4
set -euo pipefail
cd "$(dirname "$0")/../.."
FRAMES=/tmp/opencode/promo2/frames
mkdir -p "$FRAMES" dist/promo
A=scripts/promo-p5/assets
[ -f "$A/bg.mpc" ] || {
  magick -size 1920x1080 xc:"#0A0F24" "$A/vignette.png" -gravity center -composite "$A/bg.png"
  magick "$A/bg.png" "$A/bg.mpc"
}
python3 scripts/promo-p5/render.py 0 1799
ffmpeg -y -v error -framerate 30 -i "$FRAMES/%04d.jpg" -c:v libx264 -preset medium -crf 18 -pix_fmt yuv420p "$FRAMES/video-silent.mp4"
python3 scripts/promo-p5/music.py "$FRAMES/music.wav"
ffmpeg -y -v error -i "$FRAMES/music.wav" -af "loudnorm=I=-16:TP=-1.5:LRA=11" -c:a aac -b:a 128k -ar 48000 "$FRAMES/music.m4a"
ffmpeg -y -v error -i "$FRAMES/video-silent.mp4" -i "$FRAMES/music.m4a" \
  -c:v copy -c:a aac -b:a 128k -movflags +faststart -shortest dist/promo/mailclient-promo-v2.mp4
ffmpeg -y -v error -ss 25 -i dist/promo/mailclient-promo-v2.mp4 -frames:v 1 dist/promo/poster-v2.png
cat > dist/promo/mailclient-promo-v2.srt <<'SRT_EOF'
1
00:00:00,000 --> 00:00:10,000
Meet mailclient - email that respects your attention.

2
00:00:10,000 --> 00:00:20,000
Three accounts, one calm place - setup takes a minute.

3
00:00:20,000 --> 00:00:30,000
Sidebar, list, reader - responsive down to narrow screens.

4
00:00:30,000 --> 00:00:40,000
Full-text search across everything - even offline.

5
00:00:40,000 --> 00:00:50,000
Expressive writing, protective reading - by default.

6
00:00:50,000 --> 00:01:00,000
mailclient - inbox, minus the chaos.
SRT_EOF
echo "DONE: dist/promo/mailclient-promo-v2.mp4"
