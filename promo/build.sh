#!/usr/bin/env bash
# mailclient promo — Manim CE scenes (real animation engine, Pango text).
# Runs in an isolated uv env (no system/project installs): uv auto-provisions
# Python + manim into its cache. Needs system Cairo/Pango + ffmpeg.
# Usage: ./promo/build.sh  -> dist/promo/mailclient-promo.mp4 (+ poster + srt)
set -euo pipefail
cd "$(dirname "$0")/.."
MEDIA=/tmp/opencode/manim-media
SDIR="$MEDIA/videos/promo/1080p30"
mkdir -p dist/promo
uv run --with manim -- manim render -r 1920,1080 --fps 30 \
  --media_dir "$MEDIA" --format mp4 promo/promo.py S1 S2 S3 S4 S5 S6
printf "file '%s'\n" "$SDIR/S1.mp4" "$SDIR/S2.mp4" "$SDIR/S3.mp4" \
  "$SDIR/S4.mp4" "$SDIR/S5.mp4" "$SDIR/S6.mp4" > "$MEDIA/list.txt"
ffmpeg -y -v error -f concat -safe 0 -i "$MEDIA/list.txt" -c copy "$MEDIA/video-silent.mp4"
python3 promo/music.py "$MEDIA/music.wav"
ffmpeg -y -v error -i "$MEDIA/music.wav" -af "loudnorm=I=-16:TP=-1.5:LRA=11" \
  -c:a aac -b:a 128k -ar 48000 "$MEDIA/music.m4a"
ffmpeg -y -v error -i "$MEDIA/video-silent.mp4" -i "$MEDIA/music.m4a" \
  -c:v copy -c:a aac -b:a 128k -movflags +faststart -shortest dist/promo/mailclient-promo.mp4
ffmpeg -y -v error -ss 25 -i dist/promo/mailclient-promo.mp4 -frames:v 1 dist/promo/poster.png
cat > dist/promo/mailclient-promo.srt <<'SRT_EOF'
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
echo "DONE: dist/promo/mailclient-promo.mp4"
