#!/usr/bin/env python3
"""Full-feature QA for a built as-vim binary, driven through a real PTY with
screen assertions via pyte.

Usage:
    cargo build --release
    python3 -m venv .venv && .venv/bin/pip install pyte
    AS_VIM_BIN=target/release/as-vim .venv/bin/python scripts/qa_pty.py

Every scenario opens the real binary in a pseudo-terminal, types real
keystrokes (with ESC-timing handled), and asserts on saved file contents,
the emulated screen (pyte), cursor position, and raw output (OSC 52, SGR
colors, alternate-screen restore)."""

import base64, fcntl, os, pty, re, select, struct, subprocess, sys, termios, time

BIN = os.path.abspath(os.environ.get("AS_VIM_BIN", "target/release/as-vim"))
WORK = os.environ.get("AS_VIM_WORK", "/tmp/opencode/qa/work")
ROWS, COLS = 24, 80

import pyte


class Session:
    def __init__(self, args):
        self.file = os.path.join(WORK, args.file) if args.file else None
        argv = [BIN] + ([args.file] if args.file else [])
        self.pid, self.fd = pty.fork()
        if self.pid == 0:
            os.environ["TERM"] = "xterm-256color"
            os.chdir(WORK)
            os.execvp(argv[0], argv)
        fcntl.ioctl(self.fd, termios.TIOCSWINSZ, struct.pack("HHHH", ROWS, COLS, 0, 0))
        self.screen = pyte.Screen(COLS, ROWS)
        self.stream = pyte.ByteStream(self.screen)
        self.raw = b""
        self.alive = True

    def pump(self, duration=0.12):
        end = time.time() + duration
        while time.time() < end:
            r, _, _ = select.select([self.fd], [], [], 0.02)
            if r:
                try:
                    data = os.read(self.fd, 65536)
                except OSError:
                    self.alive = False
                    return
                if not data:
                    self.alive = False
                    return
                self.raw += data
                self.stream.feed(data)

    def send(self, data, settle=0.12):
        if isinstance(data, str):
            data = data.encode()
        os.write(self.fd, data)
        self.pump(settle)

    def type(self, text, settle=0.12):
        # split on ESC so bare-Esc gets breathing room from crossterm's
        # alt-key detection
        parts = text.split("\x1b")
        for i, part in enumerate(parts):
            if i > 0:
                os.write(self.fd, b"\x1b")
                self.pump(0.18)
            if part:
                os.write(self.fd, part.encode())
                self.pump(settle)

    def resize(self, rows, cols):
        fcntl.ioctl(self.fd, termios.TIOCSWINSZ, struct.pack("HHHH", rows, cols, 0, 0))
        self.screen.resize(rows, cols)
        self.pump(0.25)

    def quit_save(self, force=False):
        self.type(":" + ("wq" if not force else "q!"))
        self.send("\r", settle=0.3)

    def wait_exit(self, timeout=5):
        end = time.time() + timeout
        while time.time() < end:
            self.pump(0.05)
            done, _ = os.waitpid(self.pid, os.WNOHANG)
            if done:
                return True
        os.kill(self.pid, 9)
        os.waitpid(self.pid, 0)
        return False

    def display(self):
        return list(self.screen.display)

    def status_row(self):
        return self.screen.display[ROWS - 2].rstrip()

    def message_row(self):
        return self.screen.display[ROWS - 1].rstrip()

    def osc52_payloads(self):
        return [
            base64.b64decode(m + b"=" * (-len(m) % 4))
            for m in re.findall(rb"\x1b\]52;c;([A-Za-z0-9+/=]+)", self.raw)
        ]


def read(path):
    with open(path) as f:
        return f.read()


results = []


def check(name, cond, detail=""):
    results.append((name, bool(cond), detail))


def scenario(name, file, steps, file_expect=None, screen_checks=None, raw_checks=None):
    s = Session(type("A", (), {"file": file})())
    s.pump(0.3)
    try:
        steps(s)
        if file_expect is not None and s.file:
            check(
                f"{name}: file content",
                read(s.file) == file_expect,
                repr(read(s.file))[:120] + " != " + repr(file_expect)[:120],
            )
        if screen_checks:
            for label, fn in screen_checks:
                try:
                    ok, detail = fn(s)
                except Exception as e:
                    ok, detail = False, f"exception {e}"
                check(f"{name}: {label}", ok, detail)
        if raw_checks:
            for label, fn in raw_checks:
                ok, detail = fn(s)
                check(f"{name}: {label}", ok, detail)
    finally:
        if s.alive:
            s.wait_exit()


# ---------------------------------------------------------------- prep files

os.makedirs(WORK, exist_ok=True)
for stale in os.listdir(WORK):
    os.remove(os.path.join(WORK, stale))
with open(os.path.join(WORK, "big.txt"), "w") as f:
    for i in range(100):
        f.write(f"line{i:03d} padding padding\n")
with open(os.path.join(WORK, "hscroll.txt"), "w") as f:
    f.write(("x" * 195) + "tail\n")
with open(os.path.join(WORK, "crlf.txt"), "wb") as f:
    f.write(b"one\r\ntwo\r\n")
with open(os.path.join(WORK, "sub.txt"), "w") as f:
    f.write("one one\ntwo one\n")
with open(os.path.join(WORK, "subline.txt"), "w") as f:
    f.write("war peace peace\n")


# ---------------------------------------------------------------- scenarios


def t_basic_edit(s):
    s.type("ihello world")
    s.type("\x1b")
    s.quit_save()


scenario("basic insert+save", "basic.txt", t_basic_edit, file_expect="hello world\n")


def t_enter_split_join(s):
    s.type("iabc")  # "abc"
    s.send("\r")  # split -> abc / ""
    s.type("def")  # abc / def
    s.type("\x1b")  # NORMAL, cursor on 'f'
    s.type("0i")  # start of "def"
    s.send("\x7f")  # backspace joins -> abcdef
    s.type("\x1b")
    s.quit_save()


scenario(
    "enter split + backspace join",
    "split.txt",
    t_enter_split_join,
    file_expect="abcdef\n",
)


def t_motions(s):
    s.type("ione")
    s.send("\r")
    s.type("two")
    s.send("\r")
    s.type("three four")
    s.type("\x1b")
    s.type("gg")  # top
    s.type("$")  # end of "one"
    s.type("aX")  # oneX
    s.type("\x1b")
    s.type("G")  # last line
    s.type("0")
    s.type("AX")  # three fourX
    s.type("\x1b")
    s.type("gg0i")  # very top, col 0, insert
    s.type("Y")
    s.type("\x1b")
    s.quit_save()


scenario(
    "motions gg/G/0/$/a/A",
    "motions.txt",
    t_motions,
    file_expect="YoneX\ntwo\nthree fourX\n",
)


def t_x_delete(s):
    s.type("ihello")
    s.type("\x1b")
    s.type("0x")  # cursor home, deletes 'h'
    s.send("\x1b[3~")  # Delete key deletes 'e'
    s.quit_save()


scenario("x and Delete keys", "xdel.txt", t_x_delete, file_expect="llo\n")


def t_dd(s):
    s.type("ione")
    s.send("\r")
    s.type("two")
    s.send("\r")
    s.type("three")
    s.type("\x1b")
    s.type("gg")  # line 1
    s.type("j")  # line 2 "two"
    s.type("dd")  # delete it
    s.quit_save()


scenario("dd deletes line", "dd.txt", t_dd, file_expect="one\nthree\n")


def t_yank_paste_undo(s):
    s.type("ione")
    s.send("\r")
    s.type("two")
    s.type("\x1b")
    s.type("yy")  # yank "two"
    s.type("p")  # paste below
    s.type("u")  # undo paste
    s.type("\x12")  # Ctrl+r redo
    s.type("u")  # undo again
    s.type("P")  # paste above "two" -> one, two, two
    s.quit_save()


scenario(
    "yy/p/P/u/Ctrl+r", "yank.txt", t_yank_paste_undo, file_expect="one\ntwo\ntwo\n"
)


def t_dirty_guard(s):
    s.type("iprecious")
    s.type("\x1b")
    s.type(":q")  # should NOT quit
    s.send("\r")
    s.pump(0.3)
    checks = [
        ("still running", lambda s: (s.alive, "")),
        (
            "warning shown",
            lambda s: (
                "No write since last change" in s.message_row(),
                s.message_row(),
            ),
        ),
    ]
    for label, fn in checks:
        ok, detail = fn(s)
        check(f"dirty guard: {label}", ok, detail)
    s.type(":wq")
    s.send("\r")


scenario("dirty :q guard", "dirty.txt", t_dirty_guard, file_expect="precious\n")


def t_qforce(s):
    s.type("inope")
    s.type("\x1b")
    s.type(":q!")
    s.send("\r")


scenario("q! discards", "qforce.txt", t_qforce, file_expect=None)


def t_search(s):
    s.type("ifoo bar one")
    s.send("\r")
    s.type("bar two")
    s.send("\r")
    s.type("bar three")
    s.type("\x1b")
    s.type("gg0")
    s.type("/bar")
    s.send("\r")  # first match: line 0 col 4 (vim keeps it on the current line)
    check(
        "search: cursor at first match",
        (s.screen.cursor.y, s.screen.cursor.x) == (0, 4),
        f"cursor={s.screen.cursor.y},{s.screen.cursor.x}",
    )
    s.type("aX")  # insert after 'r' -> "foo barX one"
    s.type("\x1b")
    s.type("n")  # next match: wraps to line 1 col 0
    s.type("aY")
    s.type("\x1b")
    s.quit_save()


def search_screen_checks():
    return []


scenario(
    "search / and n",
    "search.txt",
    t_search,
    file_expect="foo bXar one\nbYar two\nbar three\n",
    screen_checks=search_screen_checks(),
)


def t_search_notfound(s):
    s.type("iabc")
    s.type("\x1b")
    s.type("/zzz")
    s.send("\r")
    s.pump(0.2)


def nf_checks():
    def msg(sess):
        return "Pattern not found" in sess.message_row(), sess.message_row()

    return [("not-found message", msg)]


scenario("search not found", "nf.txt", t_search_notfound, screen_checks=nf_checks())


def t_unknown_command(s):
    s.type("ihi")
    s.type("\x1b")
    s.type(":frobnicate")
    s.send("\r")
    s.pump(0.2)


def unk_checks():
    def msg(sess):
        return (
            "Not an editor command: frobnicate" in sess.message_row(),
            sess.message_row(),
        )

    return [("unknown cmd message", msg)]


scenario("unknown command", "unk.txt", t_unknown_command, screen_checks=unk_checks())


def t_w_newfile(s):
    s.type("imoved")
    s.type("\x1b")
    s.type(":w renamed.txt")
    s.send("\r")
    s.pump(0.2)
    check(
        ":w newfile: status shows new name",
        "renamed.txt" in s.status_row(),
        s.status_row(),
    )
    s.type(":wq")
    s.send("\r")


scenario(
    ":w <newfile>",
    "original.txt",
    t_w_newfile,
    file_expect=None,
)


def t_ctrl_c(s):
    s.type("ix")
    s.type("\x1b")
    s.send("\x03")  # Ctrl+C in normal -> hint message
    s.pump(0.2)
    ok = "Type :q to quit" in s.message_row()
    check("ctrl+c normal hints", ok, s.message_row())
    s.type(":q!")
    s.send("\r")


scenario("Ctrl+C normal", "ctrlc.txt", t_ctrl_c)


def t_burst(s):
    s.type("i")
    s.send(b"line with many words " * 10, settle=0.4)
    s.type("\x1b")
    s.quit_save()


scenario(
    "burst typing (paste-like)",
    "burst.txt",
    t_burst,
    file_expect="line with many words " * 10 + "\n",
)


def t_unicode(s):
    s.type("ihéllo 日本 🎉")
    s.type("\x1b")
    s.type("0")  # col 0
    s.type("lll")  # over h é l (display col 4 after é)
    s.type("aX")
    s.type("\x1b")
    s.quit_save()


scenario(
    "unicode + emoji editing", "uni.txt", t_unicode, file_expect="héllXo 日本 🎉\n"
)


def t_tabs(s):
    s.type("ia\tb")
    s.type("\x1b")
    s.quit_save()


def tab_checks():
    def tab_expanded(sess):
        # row 0: "a" then 4 spaces then "b"
        return sess.screen.display[0].startswith("a    b"), repr(
            sess.screen.display[0][:10]
        )

    return [("tab renders as 4 spaces", tab_checks and tab_expanded)]


scenario("tabs", "tabs.txt", t_tabs, file_expect="a\tb\n", screen_checks=tab_checks())


def t_syntax(s):
    s.type("i")
    s.send('let s: String = "hi"; // note')
    s.type("\x1b")
    s.quit_save()


def syntax_checks():
    def fg_colored(sess):
        ch = sess.screen.buffer[0][0]
        return ch.fg not in (None, "default"), f"fg={ch.fg}"

    return [("keyword colored", fg_colored)]


def syntax_raw():
    def has_color(sess):
        return b"\x1b[38;5;" in sess.raw, ""

    return [("SGR color emitted", has_color)]


scenario(
    "syntax highlighting (.rs)",
    "code.rs",
    t_syntax,
    screen_checks=syntax_checks(),
    raw_checks=syntax_raw(),
)


def t_syntax_py(s):
    s.type("idef main():")
    s.send("\r")
    s.type("print('hi')  # done")
    s.type("\x1b")
    s.quit_save()


scenario(
    "syntax highlighting (.py)",
    "code.py",
    t_syntax_py,
    raw_checks=[("py SGR emitted", lambda s: (b"\x1b[38;5;" in s.raw, ""))],
)


def t_crlf(s):
    path = os.path.join(WORK, "crlf.txt")
    with open(path, "wb") as f:
        f.write(b"one\r\ntwo\r\n")
    s.type("\x1b")  # just open, then save
    s.quit_save()


scenario("CRLF normalized on save", "crlf.txt", t_crlf, file_expect="one\ntwo\n")


def t_bigfile_scroll(s):
    s.pump(0.3)  # file pre-loaded below
    s.type("G")  # jump to end
    s.pump(0.3)


def big_checks():
    def last_line_visible(sess):
        return "line099" in sess.screen.display[ROWS - 3], sess.screen.display[0][:20]

    return [("G scrolls to bottom", last_line_visible)]


scenario("scrolling big file", "big.txt", t_bigfile_scroll, screen_checks=big_checks())


def t_hscroll(s):
    s.type("G$")  # single long line, cursor at col 199
    s.pump(0.2)


def hs_checks():
    def end_visible(sess):
        return "tail" in sess.screen.display[0], sess.screen.display[0][:30]

    return [("horizontal scroll shows EOL", hs_checks and end_visible)]


scenario("horizontal scroll", "hscroll.txt", t_hscroll, screen_checks=hs_checks())


def t_resize(s):
    s.type("ihello")
    s.type("\x1b")
    s.resize(30, 100)
    s.type("j")  # force redraw
    s.pump(0.2)


def rs_checks():
    def wide_status(sess):
        # status bar row is now 29 (30 rows - 2)
        row = sess.screen.display[28]
        return len(row.rstrip()) > 0 and "NORMAL" in row, repr(row[:40])

    return [("resize: status bar on new row", rs_checks and wide_checks_impl)]


def wide_checks_impl(sess):
    row = sess.screen.display[28]
    return "NORMAL" in row, repr(row[:40])


scenario("terminal resize", "resize.txt", t_resize, screen_checks=rs_checks())


def t_esc_alt_screen(s):
    s.type(":q")
    s.send("\r")


def alt_checks():
    def entered(sess):
        return b"\x1b[?1049h" in sess.raw, ""

    def left(sess):
        return b"\x1b[?1049l" in sess.raw, ""

    return [("alt screen entered", entered), ("alt screen left (restore)", left)]


scenario("alt screen enter/leave", "alt.txt", t_esc_alt_screen, raw_checks=alt_checks())


def t_osc52(s):
    s.type("icopyme")
    s.type("\x1b")
    s.type("yy")
    s.pump(0.3)
    payloads = s.osc52_payloads()
    check(
        "osc52 yank emitted",
        "copyme" in [p.decode(errors="replace") for p in payloads],
        str(payloads),
    )
    s.type(":q!")
    s.send("\r")


scenario("OSC52 clipboard", "osc.txt", t_osc52)

def t_substitute(s):
    s.pump(0.3)
    s.type(":%s/one/1/g")
    s.send("\r")
    s.pump(0.3)
    check("substitute: status count", "3 substitution(s) on 2 line(s)" in s.message_row(),
          s.message_row())
    s.type(":wq")
    s.send("\r")

scenario(":percent-s substitute", "sub.txt", t_substitute,
         file_expect="1 1\ntwo 1\n")

def t_substitute_line(s):
    s.pump(0.3)
    s.type(":s/war/peace/")
    s.send("\r")
    s.pump(0.3)
    s.type(":wq")
    s.send("\r")

scenario(":s single line", "subline.txt", t_substitute_line,
         file_expect="peace peace peace\n")

def t_status_fade(s):
    s.type("ihello")
    s.type("\x1b")
    s.type("yy")               # transient "Yanked line ..." message
    s.pump(0.3)
    check("status fade: message visible", "Yanked" in s.message_row(), s.message_row())
    time.sleep(3.4)            # longer than the 3s timeout
    s.type("j")                # any key redraws
    s.pump(0.3)
    check("status fade: message gone", "Yanked" not in s.message_row(), s.message_row())
    s.type(":q!")
    s.send("\r")

scenario("transient status fades", "fade.txt", t_status_fade)

def t_highlight_clears(s):
    s.type("ione cat two cat")
    s.type("\x1b")
    s.type("gg0")
    s.type("/cat")
    s.send("\r")
    s.pump(0.3)

    def any_bg(sess):
        has = any(
            sess.screen.buffer[r][c].bg not in (None, "default")
            for r in range(3)
            for c in range(20)
        )
        return has, ""

    check("highlight: bg after search", any_bg(s)[0])
    s.type("x")                # edit -> highlight clears
    s.pump(0.3)

    def no_bg(sess):
        has = any(
            sess.screen.buffer[r][c].bg not in (None, "default")
            for r in range(3)
            for c in range(20)
        )
        return not has, ""
    check("highlight: bg gone after edit", no_bg(s)[0])
    s.type(":q!")
    s.send("\r")

scenario("search highlight clears on edit", "hl.txt", t_highlight_clears)

# ---------------------------------------------------------------- run all

# ---------------------------------------------------------------- run all

failed = 0
for name, ok, detail in results:
    mark = "PASS" if ok else "FAIL"
    if not ok:
        failed += 1
    print(f"{mark:4} {name}" + (f"  -- {detail}" if detail and not ok else ""))

print(f"\n{len(results) - failed}/{len(results)} checks passed")
sys.exit(1 if failed else 0)
