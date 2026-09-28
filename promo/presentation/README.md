# Explainer video

Animated, narrated video with a talking avatar and subtitles baked in. Built with the
explainer-video kit: `engine/` is the kit's and gets replaced on upgrade — edit only
`script.mjs` (what is said), `deck/index.html` (what is shown) and `deck/custom.css`.

```bash
bun install          # Playwright, for rendering
bun run voice        # narration → build/narration.wav, build/timeline.js, subtitles
bun run preview      # play it live in the browser
bun run check        # a still per scene in build/stills/ + layout warnings
bun run render       # → build/<name>.mp4 (1080p, subtitles baked in; build/subtitles.srt beside it)
bun run build        # voice + render
```

Needs Bun, ffmpeg, and uv (or a native Python 3.10–3.12). The voice is Kokoro, local and
offline: the first run on a machine sets it up once in a shared per-user cache
(`%LOCALAPPDATA%\explainer-video` on Windows), later runs reuse it. Rendering uses
Playwright's Chromium if installed, else Edge or Chrome.

Render options: `-- --nosubs`, `-- --from 40 --to 60`, `-- --stills 3,40`, `-- --fps 60`.
Live preview keys: Space pause, ← → scenes, C subtitles, F fullscreen; `index.html#<scene>` jumps.
