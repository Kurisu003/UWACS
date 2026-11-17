#!/usr/bin/env python3
"""Quick and dirty: find out which HID button number each physical MFD button sends.

Shows the MFD with 9 buttons per side, highlights one, waits for you to press it on the real MFD.
Space = this spot has no button (skip), Backspace = go back one.
Result: ~/src/uwacs/assets/mfd_capture.json
"""
import glob, json, os, queue, threading, time, tkinter as tk

OUT = os.path.expanduser("~/src/uwacs/assets/mfd_capture.json")
IGNORE = set(range(45, 51))  # side switch + wheel, already known

def find_mfd():
    for d in sorted(glob.glob("/sys/class/hidraw/hidraw*")):
        try:
            ue = open(d + "/device/uevent").read()
        except OSError:
            continue
        if "00004098" in ue.upper() and ("0000BEE" in ue.upper()):
            return "/dev/" + os.path.basename(d)
    return None

def reader(path, q):
    fd = os.open(path, os.O_RDONLY)
    prev = set()
    while True:
        r = os.read(fd, 64)
        if not r or r[0] != 1:
            continue
        down = {i * 8 + b + 1 for i in range(7) for b in range(8) if i + 1 < len(r) and r[1 + i] >> b & 1}
        down = {u for u in down if u <= 50}
        for u in sorted(down - prev):
            if u not in IGNORE:
                q.put(u)
        prev = down

# 36 spots, clockwise from the top-left
SPOTS = [("top", i) for i in range(9)] + [("right", i) for i in range(9)] + [("bottom", 8 - i) for i in range(9)] + [("left", 8 - i) for i in range(9)]
SIDE_NAME = {"top": "TOP edge", "right": "RIGHT edge", "bottom": "BOTTOM edge", "left": "LEFT edge"}

def spot_text(s):
    side, i = s
    if side in ("top", "bottom"):
        return f"{SIDE_NAME[side]}, {i + 1}. from the LEFT"
    return f"{SIDE_NAME[side]}, {i + 1}. from the TOP"

def spot_rect(s, W=640, M=40, B=46):
    side, i = s
    inner = W - 2 * M
    step = inner / 9
    c = M + step * (i + 0.5)
    if side == "top":
        return c - B / 2 + 2, 6, c + B / 2 - 2, 6 + B * 0.7
    if side == "bottom":
        return c - B / 2 + 2, W - 6 - B * 0.7, c + B / 2 - 2, W - 6
    if side == "left":
        return 6, c - B / 2 + 2, 6 + B * 0.7, c + B / 2 - 2
    return W - 6 - B * 0.7, c - B / 2 + 2, W - 6, c + B / 2 - 2

def main():
    path = find_mfd()
    root = tk.Tk()
    root.title("MFD button capture")
    root.configure(bg="#202020")
    if not path:
        tk.Label(root, text="No WinWing MFD found", fg="white", bg="#202020", font=("Sans", 20)).pack(padx=40, pady=40)
        root.mainloop()
        return
    q = queue.Queue()
    threading.Thread(target=reader, args=(path, q), daemon=True).start()

    W = 640
    title = tk.Label(root, fg="#f0b030", bg="#202020", font=("Sans", 20, "bold"))
    title.pack(pady=(14, 2))
    sub = tk.Label(root, text="Press that button on the MFD.   Space = no button there   Backspace = back", fg="#b0b0b0", bg="#202020", font=("Sans", 12))
    sub.pack()
    cv = tk.Canvas(root, width=W, height=W, bg="#202020", highlightthickness=0)
    cv.pack(padx=20, pady=14)
    status = tk.Label(root, fg="#80d090", bg="#202020", font=("Sans", 13))
    status.pack(pady=(0, 14))

    result = {}  # index -> usage or None
    state = {"i": 0, "extras": [], "done": False}

    def save():
        out = {"spots": [], "extras": state["extras"]}
        for n, s in enumerate(SPOTS):
            out["spots"].append({"side": s[0], "index": s[1], "button": result.get(n)})
        os.makedirs(os.path.dirname(OUT), exist_ok=True)
        json.dump(out, open(OUT, "w"), indent=1)

    def draw():
        cv.delete("all")
        cv.create_rectangle(40, 40, W - 40, W - 40, outline="#555", width=2, fill="#141a16")
        for n, s in enumerate(SPOTS):
            x0, y0, x1, y1 = spot_rect(s)
            cur = n == state["i"] and not state["done"]
            got = result.get(n, "?") if n in result else None
            fill = "#f0b030" if cur else ("#2e6e40" if got not in (None, "?") else ("#3a3a3a" if n in result else "#4a4a4a"))
            cv.create_rectangle(x0, y0, x1, y1, fill=fill, outline="#4aa0ff" if cur else "#777", width=3 if cur else 1)
            label = "" if n not in result else ("-" if result[n] is None else str(result[n]))
            cv.create_text((x0 + x1) / 2, (y0 + y1) / 2, text=label, fill="black" if cur else "white", font=("Sans", 11, "bold"))
        if state["done"]:
            title.config(text="Done! Saved.")
            sub.config(text="Any other buttons not on this drawing? Press them now, they get listed too. Then close the window.")
            status.config(text=f"Saved to {OUT}" + (f"   extra buttons: {state['extras']}" if state["extras"] else ""))
        else:
            title.config(text=f"{state['i'] + 1}/36:  {spot_text(SPOTS[state['i']])}")

    def advance(val):
        if state["done"]:
            return
        n = state["i"]
        dup = [k for k, v in result.items() if v == val and k != n and val is not None]
        result[n] = val
        msg = "skipped" if val is None else f"got button {val}"
        if dup:
            msg += f"   (careful: button {val} was also given to spot {dup[0] + 1})"
        status.config(text=msg, fg="#f07060" if dup else "#80d090")
        state["i"] += 1
        if state["i"] >= len(SPOTS):
            state["done"] = True
        save()
        draw()

    def back(_e=None):
        if state["done"]:
            state["done"] = False
            state["i"] = len(SPOTS)
        if state["i"] > 0:
            state["i"] -= 1
            result.pop(state["i"], None)
        status.config(text="went back one", fg="#b0b0b0")
        draw()

    def poll():
        try:
            while True:
                u = q.get_nowait()
                if state["done"]:
                    if u not in state["extras"] and u not in result.values():
                        state["extras"].append(u)
                        save()
                        draw()
                else:
                    advance(u)
        except queue.Empty:
            pass
        root.after(30, poll)

    root.bind("<space>", lambda e: advance(None))
    root.bind("<BackSpace>", back)
    draw()
    poll()
    root.mainloop()

if __name__ == "__main__":
    main()
