"""mailclient promo v3 — Manim CE scenes (real animation engine, Pango text).
Each scene is exactly 10.0s @30fps/1080p. Render:
  uv run --with manim -- manim render -r 1920,1080 --fps 30 \\
      --media_dir /tmp/opencode/manim-media --format mp4 scripts/promo-manim/promo.py S1
Pixel layout ported 1:1 from scripts/promo-p5 (v2 calibration).
Manim units: 135 px = 1 unit. X=(px-960)/135, Y=(540-py)/135."""
from manim import *
from manim.utils.rate_functions import linear, smooth, ease_out_back as overshoot

config.pixel_width = 1920
config.pixel_height = 1080
config.frame_rate = 30
config.background_color = "#0A0F24"

CYAN = "#7DD3FC"; BLUE = "#38BDF8"; MINT = "#6EE7B7"
VIOLET = "#A78BFA"; AMBER = "#FBBF24"; SUB = "#C9D4EA"; DIM = "#8A94AD"
U = 1 / 135.0
VIGNETTE = "promo/assets/vignette.png"
NAMES = ["Meet mailclient", "Effortless setup", "A calm workspace",
         "Blazing search", "Compose and read", "Get mailclient"]
CAPS = ["Meet mailclient \u2014 email that respects your attention",
        "Three accounts, one calm place \u2014 setup takes a minute",
        "Sidebar, list, reader \u2014 responsive down to narrow screens",
        "Full-text search across everything \u2014 even offline",
        "Expressive writing, protective reading \u2014 by default",
        "mailclient \u2014 inbox, minus the chaos"]


def tracked(s):
    return "\u200a\u200a".join(s)


def T(text, size, color="#FFFFFF", bold=True, kern=False):
    m = Text(tracked(text) if kern else text, font="Noto Sans",
             weight=BOLD if bold else NORMAL, font_size=size, color=color)
    # Normalize: manim's line box for font_size N is ~1.6x the v2/IM em box,
    # so scale every text to an exact target line-box height (centers stay valid).
    m.scale((size * U) / m.height)
    return m


def at_center(m, cx, cy):
    return m.move_to([(cx - 960) * U, (540 - cy) * U, 0])


def at_left(m, x, y_top, size):
    return m.move_to([(x - 960) * U + m.width / 2, (540 - y_top - size * 0.5) * U, 0])


def at_right(m, x1, y_top, size):
    return m.move_to([(x1 - 960) * U - m.width / 2, (540 - y_top - size * 0.5) * U, 0])


def card(x0, y0, x1, y1, r=22, fill_op=0.06, stroke_op=0.20):
    return RoundedRectangle(width=(x1 - x0) * U, height=(y1 - y0) * U,
                            corner_radius=r * U, fill_color=WHITE,
                            fill_opacity=fill_op, stroke_color=WHITE,
                            stroke_opacity=stroke_op, stroke_width=2
                            ).move_to([((x0 + x1) / 2 - 960) * U, (540 - (y0 + y1) / 2) * U, 0])


def bar(x0, y0, x1, y1, color, op=1.0, r=7):
    rr = min(r, (y1 - y0) / 2)
    return RoundedRectangle(width=(x1 - x0) * U, height=(y1 - y0) * U,
                            corner_radius=rr * U, fill_color=color,
                            fill_opacity=op, stroke_width=0
                            ).move_to([((x0 + x1) / 2 - 960) * U, (540 - (y0 + y1) / 2) * U, 0])


def bar_rect(frac, color=BLUE, op=1.0):
    w = 14.2222 * frac
    return Rectangle(width=w, height=5 * U, fill_color=color, fill_opacity=op,
                     stroke_width=0).move_to([-7.1111 + w / 2, 4 - 2.5 * U, 0])


class Seg:
    """Per-scene director: chrome + progress bar synced into every play."""

    def __init__(self, scene, idx):
        self.s = scene
        self.idx = idx
        self.t = 0.0
        self.g = VGroup()
        vig = ImageMobject(VIGNETTE)
        vig.height = 8
        scene.add(vig)
        brand = T("mailclient", 34, CYAN)
        at_left(brand, 80, 44, 34)
        tag = T(f"0{idx + 1} / 06 \u00b7 {NAMES[idx]}", 28, DIM, bold=False)
        at_right(tag, 1920 - 80, 48, 28)
        cap = T(CAPS[idx], 30, bold=False)
        pill = RoundedRectangle(width=cap.width + 84 * U, height=66 * U,
                                corner_radius=33 * U, fill_color="#040814",
                                fill_opacity=0.6, stroke_color=WHITE,
                                stroke_opacity=0.3, stroke_width=2)
        at_center(pill, 960, 945)
        at_center(cap, 960, 945)
        ghost = Text(f"0{idx + 1}", font="Noto Sans", weight=BOLD, font_size=380,
                     fill_opacity=0, stroke_color=WHITE, stroke_opacity=0.05,
                     stroke_width=5)
        at_center(ghost, 1920 - 140 - ghost.width * 135 / 2, 530)
        track = Rectangle(width=14.2222, height=5 * U, fill_color=WHITE,
                          fill_opacity=0.1, stroke_width=0).move_to([0, 4 - 2.5 * U, 0])
        self.bar = bar_rect(idx / 6.0)
        scene.add(track, self.bar, brand, tag, pill, cap, ghost)

    def play(self, *anims, dt, **kw):
        t1 = self.t + dt
        f1 = (self.idx * 10 + t1) / 60.0
        self.s.play(Transform(self.bar, bar_rect(f1)), *anims, run_time=dt, **kw)
        self.t = t1

    def wait(self, dt):
        self.play(dt=dt)

    def outro(self):
        self.play(FadeOut(self.g), dt=0.6)
        assert abs(self.t - 10.0) < 1e-9, f"scene {self.idx} = {self.t}s, want 10.0"


# ---------------- scene 1: hook ----------------
class S1(Scene):
    def construct(self):
        d = Seg(self, 0)
        eb = T("A DESKTOP MAIL CLIENT FOR LINUX", 30, CYAN, kern=True)
        at_center(eb, 960, 208)
        t1 = T("Inbox, minus", 104)
        at_center(t1, 960, 345)
        t2 = T("the chaos.", 104)
        at_center(t2, 960, 455)
        ul = bar(810, 538, 1110, 545, BLUE)
        sub = T("Rust core  \u00b7  SQLite cache  \u00b7  Qt Quick interface", 36, SUB, bold=False)
        at_center(sub, 960, 600)
        panes = VGroup()
        for px, pw in [(470, 300), (790, 340), (1130, 300)]:
            g = VGroup(card(px, 648, px + pw, 866),
                       bar(px + 30, 688, px + pw - 30, 710, BLUE if px == 790 else WHITE, 1.0 if px == 790 else 0.27),
                       bar(px + 30, 728, px + 30 + (pw - 60) * 0.72, 746, WHITE, 0.16),
                       bar(px + 30, 760, px + 30 + (pw - 60) * 0.55, 778, WHITE, 0.16))
            panes.add(g)
        d1 = VGroup(*[bar(505, 688, 520, 703, CYAN), bar(825, 728, 840, 743, CYAN)])
        d.play(LaggedStart(FadeIn(eb, shift=DOWN * 0.25), FadeIn(t1, shift=DOWN * 0.35),
                           FadeIn(t2, shift=DOWN * 0.35), lag_ratio=0.2), dt=1.4)
        d.play(Create(ul), dt=0.8)
        d.play(FadeIn(sub, shift=UP * 0.2), dt=0.6)
        d.play(LaggedStart(*[FadeIn(p, shift=DOWN * 0.5) for p in panes], lag_ratio=0.25), dt=1.6)
        d.play(FadeIn(d1), dt=0.6)
        d.wait(4.4)
        d.g.add(eb, t1, t2, ul, sub, panes, d1)
        d.outro()


# ---------------- scene 2: setup ----------------
class S2(Scene):
    def construct(self):
        d = Seg(self, 1)
        eb = T("MULTI-ACCOUNT IMAP + SMTP", 30, CYAN, kern=True)
        at_center(eb, 960, 150)
        title = T("Set up in seconds.", 100)
        at_center(title, 960, 306)
        steps = VGroup()
        for i, s in enumerate(["Add an account \u2014 host, port, encryption",
                               "Folders map themselves \u2014 Inbox, Sent, Drafts, Archive",
                               "Passwords live in the OS keyring \u2014 never in the database"]):
            chip = bar(300, 408 + i * 78, 364, 472 + i * 78, BLUE, 0.27, r=14)
            num = T(str(i + 1), 34, CYAN)
            at_center(num, 332, 442 + i * 78)
            txt = T(s, 37, bold=False)
            at_left(txt, 392, 410 + i * 78, 37)
            steps.add(VGroup(chip, num, txt))
        cards = VGroup()
        for i, (nm, col) in enumerate([("you@example.com", BLUE), ("work account", VIOLET), ("side project", MINT)]):
            c = VGroup(card(340 + i * 426, 716, 340 + i * 426 + 386, 824),
                       bar(340 + i * 426, 716, 340 + i * 426 + 386, 824, col, 0.18))
            lbl = T(nm, 33)
            at_center(lbl, 340 + i * 426 + 193, 771)
            c.add(lbl)
            cards.add(c)
        d.play(LaggedStart(FadeIn(eb, shift=DOWN * 0.25), FadeIn(title, shift=DOWN * 0.35), lag_ratio=0.25), dt=1.2)
        d.play(LaggedStart(*[FadeIn(s, shift=DOWN * 0.25) for s in steps], lag_ratio=0.3), dt=1.6)
        d.play(LaggedStart(*[GrowFromCenter(c, rate_func=overshoot) for c in cards], lag_ratio=0.3), dt=1.8)
        d.wait(4.8)
        d.g.add(eb, title, steps, cards)
        d.outro()


# ---------------- scene 3: workspace ----------------
class S3(Scene):
    def construct(self):
        d = Seg(self, 2)
        eb = T("SIDEBAR  \u00b7  LIST  \u00b7  READER", 30, CYAN, kern=True)
        at_center(eb, 960, 118)
        title = T("Three panes. Zero noise.", 92)
        at_center(title, 960, 261)
        mock = VGroup(card(150, 330, 470, 850))
        folders = T("Folders", 28, CYAN)
        at_left(folders, 182, 348, 28)
        mock.add(folders)
        for i, fn in enumerate(["Inbox", "Sent", "Drafts", "Archive", "Trash"]):
            if i == 0:
                mock.add(bar(150, 396, 470, 452, BLUE, 0.23, r=12))
            f = T(fn, 29, "#FFFFFF" if i == 0 else SUB, bold=(i == 0))
            at_left(f, 182, 398 + i * 62, 29)
            mock.add(f)
        p12 = T("12", 24)
        at_center(p12, 415, 426)
        mock.add(bar(392, 410, 438, 440, BLUE, 0.9, r=8), p12)
        p3 = T("3", 24)
        at_center(p3, 411, 550)
        mock.add(bar(392, 534, 430, 564, VIOLET, 0.78, r=8), p3)
        mock.add(card(494, 330, 1134, 850))
        rows = ["Quarterly invoice attached", "Re: launch plan Friday", "Photos from the cabin trip",
                "Your receipt from Example", "Welcome to the beta group"]
        for i, s in enumerate(rows):
            dot = bar(526, 372 + i * 96, 541, 387 + i * 96, CYAN, 1.0 if i < 2 else 0.35)
            subj = T(s, 28, "#FFFFFF" if i < 4 else DIM, bold=(i < 2))
            at_left(subj, 556, 362 + i * 96, 28)
            snip = bar(556, 404 + i * 96, 556 + 300 - i * 22, 419 + i * 96, WHITE, 0.17)
            mock.add(dot, subj, snip)
        hl = VGroup(bar(494, 352, 1134, 440, BLUE, 0.26, r=14),
                    bar(494, 352, 501, 440, BLUE, 1.0, r=3))
        mock.add(hl)
        mock.add(card(1158, 330, 1770, 850))
        rtitle = bar(1194, 362, 1574, 394, WHITE, 0.35, r=8)
        rsub = bar(1194, 408, 1434, 428, WHITE, 0.20, r=8)
        rlines = VGroup(*[bar(1194, 452 + i * 34, 1194 + (520 if i < 3 else 350), 467 + i * 34, WHITE, 0.16) for i in range(4)])
        banner = bar(1194, 620, 1734, 686, AMBER, 0.23, r=14)
        btxt = T("Remote images blocked \u2014 show once", 26, AMBER, bold=False)
        at_center(btxt, 1464, 654)
        bopen = bar(1194, 716, 1344, 762, BLUE, 0.47, r=12)
        bsave = bar(1358, 716, 1508, 762, WHITE, 0.16, r=12)
        topen = T("Open", 26)
        at_center(topen, 1269, 740)
        tsave = T("Save", 26, SUB, bold=False)
        at_center(tsave, 1433, 740)
        mock.add(rtitle, rsub, rlines, banner, btxt, bopen, bsave, topen, tsave)
        d.play(LaggedStart(FadeIn(eb, shift=DOWN * 0.25), FadeIn(title, shift=DOWN * 0.35), lag_ratio=0.25), dt=1.2)
        d.play(FadeIn(mock, shift=DOWN * 0.4), dt=1.0)
        for k in range(1, 5):
            yy = 352 + ((k) % 5) * 96
            tgt = VGroup(bar(494, yy, 1134, yy + 88, BLUE, 0.26, r=14),
                         bar(494, yy, 501, yy + 88, BLUE, 1.0, r=3))
            d.play(Transform(hl, tgt), dt=1.5, rate_func=smooth)
        d.wait(1.2)
        d.g.add(eb, title, mock)
        d.outro()


# ---------------- scene 4: search ----------------
class S4(Scene):
    def construct(self):
        d = Seg(self, 3)
        eb = T("OFFLINE-FIRST SQLITE CACHE", 30, CYAN, kern=True)
        at_center(eb, 960, 150)
        title = T("Find anything, instantly.", 100)
        at_center(title, 960, 306)
        box = card(460, 400, 1460, 508)
        query = T("invoice", 52, bold=False)
        at_left(query, 510, 414, 52)
        caret = bar(510 + query.width * 135 + 8, 428, 510 + query.width * 135 + 13, 484, CYAN)
        hits = T("128 hits \u00b7 0.02 s", 40, MINT)
        at_left(hits, 480, 540, 40)
        rows = VGroup()
        for i, s in enumerate(["Quarterly invoice attached \u2014 Today", "Invoice #2418 \u2014 Tuesday",
                               "Re: invoice correction \u2014 Monday"]):
            g = VGroup(bar(460, 590 + i * 78, 1460, 660 + i * 78, WHITE, 0.09, r=16),
                       bar(492, 612 + i * 78, 505, 625 + i * 78, BLUE, 1.0, r=6))
            rt = T(s, 29, bold=False)
            at_left(rt, 520, 598 + i * 78, 29)
            g.add(rt)
            rows.add(g)
        expl = T("Type 3 letters \u2014 the FTS index answers over subject, sender, body", 33, SUB, bold=False)
        at_center(expl, 960, 862)
        d.play(LaggedStart(FadeIn(eb, shift=DOWN * 0.25), FadeIn(title, shift=DOWN * 0.35), lag_ratio=0.25), dt=1.2)
        d.play(FadeIn(box, shift=DOWN * 0.2), dt=0.6)
        self.add(caret)
        d.play(AddTextLetterByLetter(query, run_time=2.2, rate_func=linear), dt=2.2)
        d.play(FadeOut(caret), dt=0.2)
        d.play(LaggedStart(FadeIn(hits, shift=UP * 0.15), *[FadeIn(r, shift=DOWN * 0.2) for r in rows], lag_ratio=0.2), dt=1.6)
        d.play(FadeIn(expl, shift=UP * 0.2), dt=0.6)
        d.wait(3.0)
        d.g.add(eb, title, box, query, hits, rows, expl)
        d.outro()


# ---------------- scene 5: compose + read ----------------
def side_card(x0, head, hcolor, lines):
    g = VGroup(card(x0, 350, x0 + 780, 850))
    h = T(head, 30, hcolor)
    at_left(h, x0 + 44, 376, 30)
    g.add(h)
    for i, (s, cc) in enumerate(lines):
        g.add(bar(x0 + 44, 446 + i * 56, x0 + 58, 460 + i * 56, cc))
        t = T(s, 30, bold=False)
        at_left(t, x0 + 74, 438 + i * 56, 30)
        g.add(t)
    return g


class S5(Scene):
    def construct(self):
        d = Seg(self, 4)
        eb = T("COMPOSE  \u00b7  READ", 30, CYAN, kern=True)
        at_center(eb, 960, 130)
        title = T("Write and read with confidence.", 88)
        at_center(title, 960, 211)
        left_card = side_card(160, "COMPOSE", CYAN,
                              [("Rich-text editor, attachments, drafts", CYAN),
                               ("Smart send format, plain twin optional", CYAN),
                               ("From-domain guard keeps SPF, DKIM", CYAN),
                               ("and DMARC aligned", CYAN),
                               ("Queued locally \u2014 sends even if", MINT),
                               ("you close the window", MINT)])
        right_card = side_card(980, "READ SAFELY", MINT,
                               [("Sanitized HTML, remote images blocked", MINT),
                                ("Link-verify dialog before opening", MINT),
                                ("Reply-To shown inline \u2014 no surprises", MINT),
                                ("Raw headers on demand", MINT),
                                ("Attachments download on demand,", CYAN),
                                ("then stay cached offline", CYAN)])
        d.play(LaggedStart(FadeIn(eb, shift=DOWN * 0.25), FadeIn(title, shift=DOWN * 0.35), lag_ratio=0.25), dt=1.2)
        d.play(LaggedStart(FadeIn(left_card, shift=RIGHT * 0.5), FadeIn(right_card, shift=LEFT * 0.5),
                           lag_ratio=0.3), dt=1.6)
        d.wait(6.6)
        d.g.add(eb, title, left_card, right_card)
        d.outro()


# ---------------- scene 6: sync + outro ----------------
class S6(Scene):
    def construct(self):
        d = Seg(self, 5)
        eb = T("BACKGROUND SYNC + OMARCHY WIDGET", 30, CYAN, kern=True)
        at_center(eb, 960, 140)
        title = T("Quietly in sync.", 100)
        at_center(title, 960, 294)
        bullets = VGroup()
        for i, s in enumerate(["Startup, folder-open and background polling",
                               "Omarchy bar widget with unread badge",
                               "Headless --sync-once and --status JSON for scripts"]):
            dot = bar(400, 392 + i * 66, 414, 406 + i * 66, MINT)
            t = T(s, 34, bold=False)
            at_left(t, 430, 384 + i * 66, 34)
            bullets.add(VGroup(dot, t))
        glow = bar(490, 600, 1430, 720, MINT, 0.10, r=60)
        cta = RoundedRectangle(width=940 * U, height=120 * U, corner_radius=60 * U,
                               fill_opacity=0, stroke_color=MINT, stroke_opacity=0.85,
                               stroke_width=3).move_to([(960 - 960) * U, (540 - 660) * U, 0])
        ctatext = T("Free and open source", 44)
        at_center(ctatext, 960, 662)
        word = T("mailclient", 96, CYAN)
        at_center(word, 960, 775)
        tech = T("Rust  \u00b7  Qt 6  \u00b7  SQLite  \u00b7  Omarchy first", 32, SUB, bold=False)
        at_center(tech, 960, 865)
        d.play(LaggedStart(FadeIn(eb, shift=DOWN * 0.25), FadeIn(title, shift=DOWN * 0.35), lag_ratio=0.25), dt=1.2)
        d.play(LaggedStart(*[FadeIn(b, shift=DOWN * 0.2) for b in bullets], lag_ratio=0.3), dt=1.6)
        d.play(LaggedStart(FadeIn(glow), FadeIn(cta), FadeIn(ctatext, shift=UP * 0.15), lag_ratio=0.2), dt=1.2)
        d.play(LaggedStart(FadeIn(word, shift=DOWN * 0.25), FadeIn(tech, shift=UP * 0.15), lag_ratio=0.3), dt=1.0)
        d.wait(2.4)
        d.play(glow.animate.set_fill(opacity=0.22), dt=0.7)
        d.play(glow.animate.set_fill(opacity=0.10), dt=0.7)
        d.wait(0.6)
        d.g.add(eb, title, bullets, glow, cta, ctatext, word, tech)
        d.outro()
