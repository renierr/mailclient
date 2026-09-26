#!/usr/bin/env python3
"""Deterministic promo renderer via ImageMagick (no browser/GPU needed).
Port of sketch.js/promo.qml design: static navy bg, eased kinetic type,
UI mockups, 1800 frames @30fps. Usage: render.py [start end] | render.py --cal F"""
import math, os, subprocess, sys
from concurrent.futures import ThreadPoolExecutor

HERE = os.path.dirname(os.path.abspath(__file__))
BG = os.path.join(HERE, "assets", "bg.mpc")
OUT = "/tmp/opencode/promo2/frames"
CAL = "/tmp/opencode/cal.jpg"
FPS, SCENE_LEN, TOTAL = 30, 300, 1800
FB, FR = "Noto-Sans-Bold", "Noto-Sans-Regular"
INK, SUB, DIM = "#FFFFFF", "#C9D4EA", "#8A94AD"
CYAN, BLUE, MINT, VIOLET, AMBER = "#7DD3FC", "#38BDF8", "#6EE7B7", "#A78BFA", "#FBBF24"
NAMES = ["Meet mailclient", "Effortless setup", "A calm workspace", "Blazing search", "Compose and read", "Get mailclient"]
CAPS = ["Meet mailclient \u2014 email that respects your attention",
        "Three accounts, one calm place \u2014 setup takes a minute",
        "Sidebar, list, reader \u2014 responsive down to narrow screens",
        "Full-text search across everything \u2014 even offline",
        "Expressive writing, protective reading \u2014 by default",
        "mailclient \u2014 inbox, minus the chaos"]

def clamp01(v): return 0.0 if v < 0 else 1.0 if v > 1 else v
def pr(x, a, b): return clamp01((x - a) / (b - a))
def eoc(t): t = clamp01(t); return 1 - (1 - t) ** 3
def eio(t): t = clamp01(t); return 4*t*t*t if t < .5 else 1 - (-2*t+2)**3/2
def eob(t):
    t = clamp01(t); c = 1.70158
    return 1 + (c+1)*(t-1)**3 + c*(t-1)**2
def rise(lt, d, dur, dist): return (1 - eoc(pr(lt, d, d+dur))) * dist
def salpha(lt): return min(eoc(pr(lt, 0, 14)), 1 - eio(pr(lt, SCENE_LEN-18, SCENE_LEN)))

def rgba(hexcol, a):
    h = hexcol.lstrip("#"); r, g, b = int(h[0:2],16), int(h[2:4],16), int(h[4:6],16)
    return f"rgba({r},{g},{b},{max(0,min(1,a)):.3f})"

WIDCACHE = {}
def textw(font, size, s):
    if not s: return 0
    k = (font, size, s)
    if k not in WIDCACHE:
        p = subprocess.run(["magick","-font",font,"-pointsize",str(size),
                            f"label:{s}","-format","%w","info:"],
                           capture_output=True, text=True)
        WIDCACHE[k] = int(p.stdout.strip())
    return WIDCACHE[k]

class F:
    def __init__(self): self.a = ["magick", BG]
    def rect(self, x0,y0,x1,y1,r,fill,stroke=None,sw=0):
        x0,y0,x1,y1 = round(x0),round(y0),round(x1),round(y1)
        self.a += ["-fill",fill]
        if stroke and sw: self.a += ["-stroke",stroke,"-strokewidth",str(sw)]
        else: self.a += ["-stroke","none"]
        self.a += ["-draw", f"roundrectangle {x0},{y0} {x1},{y1} {r},{r}"]
    def card(self, x0,y0,x1,y1,sa,r=22):
        self.rect(x0,y0,x1,y1,r,rgba("#FFFFFF",0.06*sa),rgba("#FFFFFF",0.20*sa),2)
    def text(self, font,size,fill,grav,x,y,s,kern=0):
        x, y = round(x), round(y)
        # NOTE: with -gravity, -annotate offsets are relative to the gravity
        # point (Center = image center 960,540), so sign them explicitly.
        geo = f"{x:+d}{y:+d}"
        self.a += ["-font",font,"-pointsize",str(size)]
        if kern: self.a += ["-kerning",str(kern)]
        self.a += ["-fill",fill,"-stroke","none","-gravity",grav,"-annotate",geo,s]
        if kern: self.a += ["+kerning"]
    def center(self, font,size,fill,cx,cy,s,kern=0):
        self.text(font,size,fill,"Center",cx-960,cy-540,s,kern)
    def left(self, font,size,fill,x,y,s):
        self.text(font,size,fill,"NorthWest",x,y,s)
    def ghost(self, num, sa, size=380):
        w = textw(FB,size,num)
        self.a += ["-font",FB,"-pointsize",str(size),"-fill","none",
                   "-stroke",rgba("#FFFFFF",0.05*sa),"-strokewidth","3",
                   "-gravity","NorthWest","-annotate",f"+{1920-100-w}+340",num]

def chrome(cmd, f, sc):
    pf = f/(TOTAL-1)
    cmd.rect(0,0,1920,5,0,rgba("#FFFFFF",0.10))
    cmd.rect(0,0,1920*pf,5,0,BLUE)
    cmd.left(FB,34,CYAN,80,44,"mailclient")
    tag = f"0{sc+1} / 06 \u00b7 {NAMES[sc]}"
    cmd.left(FR,28,DIM,1920-80-textw(FR,28,tag),48,tag)
    tw = textw(FR,30,CAPS[sc])
    cmd.rect(960-tw/2-42,912,960+tw/2+42,978,33,rgba("#040814",0.60),rgba("#FFFFFF",0.30),2)
    cmd.center(FR,30,INK,960,945,CAPS[sc])

def sc1(c, lt, sa):
    c.ghost("01",sa)
    c.center(FB,30,rgba(CYAN,sa),960,208+rise(lt,0,40,30),"A DESKTOP MAIL CLIENT FOR LINUX",kern=6)
    c.center(FB,104,rgba(INK,sa),960,315+rise(lt,8,44,44)+30,"Inbox, minus")
    c.center(FB,104,rgba(INK,sa),960,425+rise(lt,14,44,44)+30,"the chaos.")
    uw = 300+240*eoc(pr(lt,40,90))
    c.rect(960-uw/2,538,960+uw/2,545,3,rgba(BLUE,sa))
    c.center(FR,36,rgba(SUB,sa),960,586+rise(lt,40,40,26)+14,"Rust core  \u00b7  SQLite cache  \u00b7  Qt Quick interface")
    for i,(px,pw) in enumerate([(470,300),(790,340),(1130,300)]):
        d = rise(lt,70+i*14,50,120); a = pr(lt,70+i*14,110+i*14)*sa
        if a <= 0: continue
        c.card(px,648+d,px+pw,648+d+218,a)
        c.rect(px+30,688+d,px+pw-30,710+d,9,rgba(BLUE if i==1 else "#FFFFFF",(1 if i==1 else 0.27)*a))
        c.rect(px+30,728+d,px+30+(pw-60)*0.72,746+d,9,rgba("#FFFFFF",0.16*a))
        c.rect(px+30,760+d,px+30+(pw-60)*0.55,778+d,9,rgba("#FFFFFF",0.16*a))
    c.rect(505,688+rise(lt,70,50,120),520,703+rise(lt,70,50,120),7,rgba(CYAN,pr(lt,70,110)*sa))
    c.rect(825,728+rise(lt,84,50,120),840,743+rise(lt,84,50,120),7,rgba(CYAN,pr(lt,84,124)*sa))

def sc2(c, lt, sa):
    c.ghost("02",sa)
    c.center(FB,30,rgba(CYAN,sa),960,150+rise(lt,0,40,30),"MULTI-ACCOUNT IMAP + SMTP",kern=6)
    c.center(FB,100,rgba(INK,sa),960,268+rise(lt,8,44,44)+38,"Set up in seconds.")
    steps = ["Add an account \u2014 host, port, encryption",
             "Folders map themselves \u2014 Inbox, Sent, Drafts, Archive",
             "Passwords live in the OS keyring \u2014 never in the database"]
    for i,s in enumerate(steps):
        a = pr(lt,30+i*26,60+i*26)*sa; r = rise(lt,30+i*26,40,34)
        if a <= 0: continue
        c.rect(300,408+i*78+r,364,472+i*78+r,14,rgba(BLUE,0.27*a))
        c.center(FB,34,rgba(CYAN,a),332,442+i*78+r,str(i+1))
        c.left(FR,37,rgba(INK,a),392,410+i*78+r,s)
    for i,(nm,col) in enumerate([("you@example.com",BLUE),("work account",VIOLET),("side project",MINT)]):
        a = pr(lt,130+i*30,150+i*30)*sa
        if a <= 0: continue
        x = 340+i*426
        c.rect(x,716,x+386,824,22,rgba("#FFFFFF",0.06*a))
        c.rect(x,716,x+386,824,22,rgba(col,0.18*a))
        c.center(FB,33,rgba(INK,a),x+193,771,nm)

def sc3(c, lt, sa):
    c.ghost("03",sa)
    c.center(FB,30,rgba(CYAN,sa),960,118+rise(lt,0,40,30),"SIDEBAR  \u00b7  LIST  \u00b7  READER",kern=6)
    c.center(FB,92,rgba(INK,sa),960,226+rise(lt,8,44,44)+35,"Three panes. Zero noise.")
    a = pr(lt,30,80)*sa; dy = rise(lt,30,60,90)
    if a > 0:
        c.card(150,330+dy,150+320,330+dy+520,a)
        c.left(FB,28,rgba(CYAN,a),182,348+dy,"Folders")
        for i,fn in enumerate(["Inbox","Sent","Drafts","Archive","Trash"]):
            if i == 0: c.rect(150,396+dy,470,452+dy,12,rgba(BLUE,0.23*a))
            c.left(FB if i==0 else FR,29,rgba(INK if i==0 else SUB,a),182,398+dy+i*62,fn)
        c.rect(392,410+dy,438,440+dy,8,rgba(BLUE,0.90*a))
        c.center(FB,24,rgba(INK,a),415,426+dy,"12")
        c.rect(392,534+dy,430,564+dy,8,rgba(VIOLET,0.78*a))
        c.center(FB,24,rgba(INK,a),411,550+dy,"3")
        idx = (lt//45) % 5; prev = (idx+4) % 5
        hy = 352+prev*96+((352+idx*96)-(352+prev*96))*eio(pr(lt%45,0,12))
        c.card(494,330+dy,494+640,330+dy+520,a)
        c.rect(494,hy+dy,494+640,hy+dy+88,14,rgba(BLUE,0.26*a))
        c.rect(494,hy+dy,501,hy+dy+88,3,rgba(BLUE,a))
        subs = ["Quarterly invoice attached","Re: launch plan Friday","Photos from the cabin trip","Your receipt from Example","Welcome to the beta group"]
        for i,s in enumerate(subs):
            c.rect(526,372+dy+i*96,541,387+dy+i*96,7,rgba(CYAN,(1 if i<2 else 0.35)*a))
            c.left(FB if i<2 else FR,28,rgba(INK if i<4 else DIM,a),556,362+dy+i*96,s)
            c.rect(556,404+dy+i*96,556+300-i*22,419+dy+i*96,7,rgba("#FFFFFF",0.17*a))
        c.card(1158,330+dy,1158+612,330+dy+520,a)
        c.rect(1194,362+dy,1574,394+dy,8,rgba("#FFFFFF",0.35*a))
        c.rect(1194,408+dy,1434,428+dy,8,rgba("#FFFFFF",0.20*a))
        for i in range(4):
            w = 520 if i < 3 else 350
            c.rect(1194,452+dy+i*34,1194+w,467+dy+i*34,7,rgba("#FFFFFF",0.16*a))
        c.rect(1194,620+dy,1734,686+dy,14,rgba(AMBER,0.23*a))
        c.center(FR,26,rgba(AMBER,a),1464,654+dy,"Remote images blocked \u2014 show once")
        c.rect(1194,716+dy,1344,762+dy,12,rgba(BLUE,0.47*a))
        c.rect(1358,716+dy,1508,762+dy,12,rgba("#FFFFFF",0.16*a))
        c.center(FB,26,rgba(INK,a),1269,740+dy,"Open")
        c.center(FR,26,rgba(SUB,a),1433,740+dy,"Save")

def sc4(c, lt, sa):
    c.ghost("04",sa)
    c.center(FB,30,rgba(CYAN,sa),960,150+rise(lt,0,40,30),"OFFLINE-FIRST SQLITE CACHE",kern=6)
    c.center(FB,100,rgba(INK,sa),960,268+rise(lt,8,44,44)+38,"Find anything, instantly.")
    q = "invoice"; n = min(7, int(eio(pr(lt,30,110))*8)); shown = q[:n]
    c.card(460,400,460+1000,400+108,sa)
    c.left(FR,52,rgba(INK,sa),510,414,shown)
    if (lt//15) % 2 == 0 and n < 7:
        cx = 510+textw(FR,52,shown)+8
        c.rect(cx,428,cx+5,484,2,rgba(CYAN,sa))
    if n >= 7:
        a = pr(lt,120,140)*sa
        if a > 0:
            c.left(FB,40,rgba(MINT,a),480,540,"128 hits \u00b7 0.02 s")
            for i,row in enumerate(["Quarterly invoice attached \u2014 Today","Invoice #2418 \u2014 Tuesday","Re: invoice correction \u2014 Monday"]):
                aa = pr(lt,135+i*22,159+i*22)*sa; rr = rise(lt,135+i*22,30,30)
                if aa <= 0: continue
                c.rect(460,590+i*78+rr,1460,660+i*78+rr,16,rgba("#FFFFFF",0.09*aa))
                c.rect(492,612+i*78+rr,505,625+i*78+rr,6,rgba(BLUE,aa))
                c.left(FR,29,rgba(INK,aa),520,598+i*78+rr,row)
    c.center(FR,33,rgba(SUB,sa),960,849+rise(lt,150,40,26)+13,"Type 3 letters \u2014 the FTS index answers over subject, sender, body")

def sc5(c, lt, sa):
    c.ghost("05",sa)
    c.center(FB,30,rgba(CYAN,sa),960,130+rise(lt,0,40,30),"COMPOSE  \u00b7  READ",kern=6)
    c.center(FB,88,rgba(INK,sa),960,240+rise(lt,8,44,44)+33,"Write and read with confidence.")
    lx = -780+940*eoc(pr(lt,20,70)); rx = 1920-940*eoc(pr(lt,40,90))
    a = pr(lt,20,45)*sa
    if a > 0:
        c.card(lx,350,lx+780,350+500,a)
        c.left(FB,30,rgba(CYAN,a),44+lx,376,"COMPOSE")
        cl = ["Rich-text editor, attachments, drafts","Smart send format, plain twin optional","From-domain guard keeps SPF, DKIM","and DMARC aligned","Queued locally \u2014 sends even if","you close the window"]
        for i,s in enumerate(cl):
            cc = CYAN if i < 4 else MINT
            c.rect(44+lx,446+i*56,58+lx,460+i*56,7,rgba(cc,a))
            c.left(FR,30,rgba(INK,a),74+lx,438+i*56,s)
    a = pr(lt,40,65)*sa
    if a > 0:
        c.card(rx,350,rx+780,350+500,a)
        c.left(FB,30,rgba(MINT,a),44+rx,376,"READ SAFELY")
        rl = ["Sanitized HTML, remote images blocked","Link-verify dialog before opening","Reply-To shown inline \u2014 no surprises","Raw headers on demand","Attachments download on demand,","then stay cached offline"]
        for i,s in enumerate(rl):
            cc = MINT if i < 4 else CYAN
            c.rect(44+rx,446+i*56,58+rx,460+i*56,7,rgba(cc,a))
            c.left(FR,30,rgba(INK,a),74+rx,438+i*56,s)

def sc6(c, lt, sa):
    c.ghost("06",sa)
    c.center(FB,30,rgba(CYAN,sa),960,140+rise(lt,0,40,30),"BACKGROUND SYNC + OMARCHY WIDGET",kern=6)
    c.center(FB,100,rgba(INK,sa),960,256+rise(lt,8,44,44)+38,"Quietly in sync.")
    bl = ["Startup, folder-open and background polling","Omarchy bar widget with unread badge","Headless --sync-once and --status JSON for scripts"]
    for i,s in enumerate(bl):
        a = pr(lt,30+i*24,58+i*24)*sa; r = rise(lt,30+i*24,36,30)
        if a <= 0: continue
        c.rect(400,392+i*66+r,414,406+i*66+r,7,rgba(MINT,a))
        c.left(FR,34,rgba(INK,a),430,384+i*66+r,s)
    a = pr(lt,120,160)*sa
    if a > 0:
        pulse = 0.10+0.06*math.sin(lt*0.07)
        c.rect(490,600,1430,720,60,rgba(MINT,pulse*a),rgba(MINT,0.85*a),3)
        c.center(FB,44,rgba(INK,a),960,662,"Free and open \u2014 run ./dev.sh to try it")
    c.center(FB,96,rgba(CYAN,sa),960,733+rise(lt,150,44,30)+42,"mailclient")
    c.center(FR,32,rgba(SUB,sa),960,865,"Rust  \u00b7  Qt 6  \u00b7  SQLite  \u00b7  Omarchy first")

SCENES = [sc1,sc2,sc3,sc4,sc5,sc6]

def render_frame(f, out=None):
    sc = min(5, f//SCENE_LEN); lt = f-sc*SCENE_LEN
    c = F()
    SCENES[sc](c, lt, salpha(lt))
    chrome(c, f, sc)
    c.a += ["-quality","93", out or os.path.join(OUT, f"{f:04d}.jpg")]
    subprocess.run(c.a, check=True, capture_output=True)

def main():
    os.makedirs(OUT, exist_ok=True)
    args = sys.argv[1:]
    if args[:1] == ["--cal"]:
        render_frame(int(args[1]), CAL); print("cal done"); return
    start, end = (int(args[0]), int(args[1])) if len(args) >= 2 else (0, TOTAL-1)
    jobs = [f for f in range(start, end+1) if not os.path.exists(os.path.join(OUT, f"{f:04d}.jpg"))]
    print(f"rendering {len(jobs)} frames ({start}..{end})", flush=True)
    import time; t0 = time.time(); done = [0]
    def one(f):
        render_frame(f); done[0] += 1
        if done[0] % 120 == 0: print(f"  {done[0]}/{len(jobs)} ({time.time()-t0:.0f}s)", flush=True)
    with ThreadPoolExecutor(max_workers=10) as ex: list(ex.map(one, jobs))
    print(f"done in {time.time()-t0:.0f}s", flush=True)

if __name__ == "__main__": main()
