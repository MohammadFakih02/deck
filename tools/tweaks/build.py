"""Builds Deck's Tweaks list (src-tauri/tweaks/tweaks.json) and its built-in app list
(src-tauri/tweaks/apps.json).

Recipes come from two MIT-licensed projects, read at the pinned commits below, plus Deck's own:
  - Win11Debloat by Raphire  (Config/Features.json, Config/Apps.json, Regfiles/*.reg)
  - WinUtil by Chris Titus Tech (config/tweaks.json)
Both are credited in THIRD_PARTY_NOTICES.md. Nothing is taken from GPL projects (WinScript).

    python tools/tweaks/build.py            # downloads the sources once into tools/tweaks/.cache

Every tweak here is a *feature* switch: "on" is the feature on (usually Windows' default), "off" is
it off. Choices list every value. The build fails when two tweaks write the same registry value
(the later one would quietly change the earlier one's state) or a recipe is missing.
"""
import json, os, re, sys, urllib.error, urllib.request

W11D_SHA = "a2d63c7c1b9c7cb6554d4fce3aa7f377567f7f9e"
WINUTIL_SHA = "7514faf0d0f4bff9b6c5d9213426ab52520cfac4"
HERE = os.path.dirname(os.path.abspath(__file__))
CACHE = os.path.join(HERE, ".cache")
OUT = os.path.join(HERE, "..", "..", "src-tauri", "tweaks")


def fetch(url, name):
    os.makedirs(CACHE, exist_ok=True)
    p = os.path.join(CACHE, name)
    if not os.path.exists(p):
        with urllib.request.urlopen(url, timeout=60) as r:
            open(p, "wb").write(r.read())
    return open(p, "rb").read()


def w11d_file(path):
    return fetch(f"https://raw.githubusercontent.com/Raphire/Win11Debloat/{W11D_SHA}/" + path.replace(" ", "%20"), "w11d__" + path.replace("/", "__"))


HIVES = {"HKEY_CURRENT_USER": "HKCU", "HKEY_LOCAL_MACHINE": "HKLM", "HKEY_CLASSES_ROOT": "HKCR", "HKEY_USERS": "HKU"}


def parse_reg(raw):
    """A .reg file as Deck's steps."""
    for enc in ("utf-16", "utf-8-sig"):
        try:
            text = raw.decode(enc)
            if "Registry Editor" in text or "REGEDIT" in text:
                break
        except Exception:
            continue
    else:
        raise ValueError("not a .reg file")
    text = re.sub(r"\\\r?\n\s*", "", text)
    steps, key = [], None
    for line in text.splitlines():
        line = line.strip()
        if not line or line.startswith(";"):
            continue
        m = re.match(r"^\[(-?)([^\]]+)\]$", line)
        if m:
            hive, _, rest = m.group(2).partition("\\")
            key = HIVES[hive] + "\\" + rest
            if m.group(1) == "-":
                steps.append({"reg": key, "key_delete": True})
            continue
        m = re.match(r'^("(?:[^"\\]|\\.)*"|@)=(.*)$', line)
        if not m or key is None:
            continue
        name = "" if m.group(1) == "@" else m.group(1)[1:-1].replace('\\\\', '\\').replace('\\"', '"')
        v = m.group(2)
        s = {"reg": key, "name": name}
        if v == "-":
            s["delete"] = True
        elif v.startswith("dword:"):
            s.update(type="dword", value=int(v[6:], 16))
        elif v.startswith('"'):
            s.update(type="string", value=v[1:-1].replace('\\\\', '\\').replace('\\"', '"'))
        elif v.startswith("hex(b):"):
            s.update(type="qword", value=int.from_bytes(bytes(int(x, 16) for x in v[7:].split(",") if x), "little"))
        elif v.startswith("hex(2):"):
            s.update(type="expand", value=bytes(int(x, 16) for x in v[7:].split(",") if x).decode("utf-16-le").rstrip("\0"))
        elif v.startswith("hex:"):
            s.update(type="binary", value=v[4:].replace(",", "").lower())
        else:
            raise ValueError(f"unknown value {v}")
        steps.append(s)
    return steps


FEATURES = {f["FeatureId"]: f for f in json.loads(w11d_file("Config/Features.json").decode("utf-8-sig"))["Features"]}
APPS = json.loads(w11d_file("Config/Apps.json").decode("utf-8-sig"))
WINUTIL = json.loads(fetch(f"https://raw.githubusercontent.com/ChrisTitusTech/winutil/{WINUTIL_SHA}/config/tweaks.json", "winutil__tweaks.json").decode("utf-8"), strict=False)


def regfile(name, undo=False):
    if not name:
        return None
    if not name.endswith(".reg"):
        name += ".reg"
    # undo files are in Regfiles/Undo, a few in Regfiles itself; names differ in case now and then
    for folder in (["Regfiles/Undo/", "Regfiles/"] if undo else ["Regfiles/"]):
        path = W11D_FILES.get((folder + name).lower())
        if path:
            return parse_reg(w11d_file(path))
    raise SystemExit(f"Win11Debloat has no {name}")


W11D_FILES = {t["path"].lower(): t["path"] for t in json.loads(fetch(f"https://api.github.com/repos/Raphire/Win11Debloat/git/trees/{W11D_SHA}?recursive=1", "w11d__tree.json"))["tree"] if t["type"] == "blob"}


def w11d(fid):
    """(apply, undo, min build, max build) of a Win11Debloat feature."""
    f = FEATURES[fid]
    apply, undo = regfile(f.get("RegistryKey")), regfile(f.get("RegistryUndoKey"), undo=True)
    if apply is None:
        raise SystemExit(f"Win11Debloat {fid} has no registry file")
    return apply, undo or [], f.get("MinVersion"), f.get("MaxVersion")


WU_TYPES = {"DWord": "dword", "String": "string", "QWord": "qword", "ExpandString": "expand", "Binary": "binary"}


def wu_step(r, side):
    hive, _, rest = r["Path"].partition(":\\")
    s = {"reg": hive + "\\" + rest, "name": r["Name"]}
    v = r["Value"] if side == "on" else r["OriginalValue"]
    if v == "<RemoveEntry>":
        s["delete"] = True
    else:
        t = WU_TYPES[r["Type"]]
        s.update(type=t, value=int(v) if t in ("dword", "qword") else v)
    return s


def winutil(key):
    """(the tweak's steps, WinUtil's original values) of a WinUtil tweak."""
    regs = WINUTIL[key].get("registry") or []
    if not regs:
        raise SystemExit(f"WinUtil {key} has no registry steps")
    return [wu_step(r, "on") for r in regs], [wu_step(r, "off") for r in regs]


def reg(key, name, value, type="dword"):
    return {"reg": key, "name": name, "type": type, "value": value}


def gone(key, name):
    return {"reg": key, "name": name, "delete": True}


TWEAKS = []


def add(t):
    TWEAKS.append(t)
    return t


def sw(id, cat, name, what, on, off, default="on", risk="safe", after="none", src="Deck", min=None, max=None, needs=None, rec=None, more=None, group=None, labels=("On", "Off")):
    """A switch: the feature on or off."""
    return add({"id": id, "cat": cat, "group": group, "name": name, "what": what, "more": more, "risk": risk, "after": after, "src": src,
                "min": min, "max": max, "needs": needs or [], "kind": "switch", "default": default, "rec": rec,
                "options": [{"id": "on", "label": labels[0], "steps": on}, {"id": "off", "label": labels[1], "steps": off}]})


def w_off(fid, id, cat, name, what, **kw):
    """A Win11Debloat "Disable…/Hide…" feature: its undo is Windows' default (on)."""
    a, u, mn, mx = w11d(fid)
    kw.setdefault("min", mn)
    kw.setdefault("max", mx)
    kw.setdefault("src", "Win11Debloat")
    kw.setdefault("rec", None)
    if not u:
        # Win11Debloat has no undo: removed keys come back empty (the folder's entry is the key itself)
        u = [{"reg": s["reg"], "key_create": True} for s in a if s.get("key_delete")]
    return sw(id, cat, name, what, u, a, default="on", **kw)


def w_on(fid, id, cat, name, what, **kw):
    """A Win11Debloat "Enable…/Show…" feature: applying it turns the feature on."""
    a, u, mn, mx = w11d(fid)
    kw.setdefault("min", mn)
    kw.setdefault("max", mx)
    kw.setdefault("src", "Win11Debloat")
    return sw(id, cat, name, what, a, u, default="off", **kw)


def choice(id, cat, name, what, options, default, risk="safe", after="none", src="Deck", min=None, max=None, needs=None, rec=None, more=None, group=None):
    return add({"id": id, "cat": cat, "group": group, "name": name, "what": what, "more": more, "risk": risk, "after": after, "src": src,
                "min": min, "max": max, "needs": needs or [], "kind": "choice", "default": default, "rec": rec,
                "options": [{"id": oid, "label": label, "steps": steps} for oid, label, steps in options]})


def w_group(gid, id, cat, name, what, labels, **kw):
    """A Win11Debloat choice group: each value's registry file is that option."""
    g = next(x for x in json.loads(w11d_file("Config/Features.json").decode("utf-8-sig"))["UiGroups"] if x["GroupId"] == gid)
    opts, default, mn = [], None, None
    for (oid, label), v in zip(labels, g["Values"]):
        fid = v["FeatureIds"][0]
        a, _, m, _ = w11d(fid)
        mn = mn or m
        opts.append((oid, label, a))
        if "(Default)" in v["Label"]:
            default = oid
    kw.setdefault("min", mn)
    kw.setdefault("src", "Win11Debloat")
    return choice(id, cat, name, what, opts, default, **kw)


def action(id, cat, name, what, do, risk="behaviour", after="none", src="Deck", min=None, max=None, needs=None, more=None, group=None, label="Run"):
    """A one-off action (with its own undo where it has one)."""
    return add({"id": id, "cat": cat, "group": group, "name": name, "what": what, "more": more, "risk": risk, "after": after, "src": src,
                "min": min, "max": max, "needs": needs or [], "kind": "action", "do": do, "label": label, "default": None, "rec": None, "options": []})


def link(id, cat, name, what, page, label):
    """Owned by another Deck page: shown here with a link, never changed here."""
    return add({"id": id, "cat": cat, "group": None, "name": name, "what": what, "more": None, "risk": "safe", "after": "none", "src": "Deck",
                "min": None, "max": None, "needs": [], "kind": "link", "page": page, "label": label, "default": None, "rec": None, "options": []})


ADV = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced"
POL = r"HKLM\SOFTWARE\Policies\Microsoft\Windows"

# ----------------------------------------------------------------------------- Privacy
# (the more specific tweaks first: the build gives a shared value to the first one that claims it)
a, u = winutil("WPFTweaksActivity")
sw("privacy.activity", "privacy", "Activity history", "Windows records the apps and files you use and can upload the list. Clipboard history isn't affected.", u, a, rec="off", src="WinUtil", after="none")
sw("privacy.feedback", "privacy", "Feedback requests", "Windows asks you now and then to rate it or give feedback.",
   [gone(r"HKCU\Software\Microsoft\Siuf\Rules", "NumberOfSIUFInPeriod"), gone(POL + r"\DataCollection", "DoNotShowFeedbackNotifications")],
   [reg(r"HKCU\Software\Microsoft\Siuf\Rules", "NumberOfSIUFInPeriod", 0), reg(POL + r"\DataCollection", "DoNotShowFeedbackNotifications", 1)], rec="off")
w_off("DisableEdgeAds", "privacy.edge_ads", "privacy", "Ads and news in Edge", "Edge's new tab news, shopping help, recommendations and other promotions. Edge itself keeps working.", rec="off", more="These are Edge policies, so Edge then says \"Your browser is managed by your organization\": that line comes from this tweak and goes away with Undo.")
w_off("DisableTelemetry", "privacy", "privacy", "Diagnostic data and ads ID", "Windows sends more than the required diagnostic data, tailors tips to it, gives apps an advertising ID, learns from your typing and inking, and sends speech to Microsoft. Off keeps only what Windows requires.", rec="off", more="Also turns off online speech recognition and the inking & typing dictionary.")
w_off("DisableSuggestions", "privacy.suggestions", "privacy", "Tips and suggested content", "\"Get tips and tricks\", suggested apps and content in Settings and Start, and the \"finish setting up your device\" screens.", rec="off")
w_off("DisableLockscreenTips", "privacy.lockscreen_tips", "privacy", "Lock screen tips and facts", "Tips, fun facts and suggestions on the lock screen.", rec="off")
w_off("DisableSettings365Ads", "privacy.settings_ads", "privacy", "Microsoft 365 ads in Settings", "Promotions for Microsoft 365 and Copilot on Settings' Home page.", rec="off")
a, u = winutil("WPFTweaksConsumerFeatures")
sw("privacy.consumer", "privacy", "Suggested apps installed by Windows", "Windows installs promoted Store apps and games (Candy Crush and the like) on its own.", u, a, rec="off", src="WinUtil")
w_off("DisableDeviceAutoAppDownload", "privacy.device_apps", "privacy", "Companion apps for new devices", "When you plug in a device (a mouse, a monitor), Windows downloads its maker's app.", rec="off", risk="behaviour")
w_off("DisableLocationServices", "privacy.location", "privacy", "Location for Windows and apps", "Windows' location service, which apps, the weather and Find My Device use.", risk="behaviour")
w_off("DisableFindMyDevice", "privacy.findmydevice", "privacy", "Find My Device", "Windows saves this PC's location now and then so you can find it from your Microsoft account.", risk="behaviour")
SS = r"HKCU\Software\Microsoft\Windows\CurrentVersion\SearchSettings"
sw("privacy.cloud_search", "privacy", "Cloud results in search", "Search also shows files and mail from your Microsoft account and work or school account.",
   [reg(SS, "IsMSACloudSearchEnabled", 1), reg(SS, "IsAADCloudSearchEnabled", 1)], [reg(SS, "IsMSACloudSearchEnabled", 0), reg(SS, "IsAADCloudSearchEnabled", 0)], rec="off")
sw("privacy.search_history", "privacy", "Search history on this PC", "Start remembers what you searched for to suggest it again.",
   [reg(SS, "IsDeviceSearchHistoryEnabled", 1)], [reg(SS, "IsDeviceSearchHistoryEnabled", 0)])
a, u = winutil("WPFTweaksWPBT")
sw("privacy.wpbt", "privacy", "Firmware programs at start-up (WPBT)", "Lets the PC's firmware make Windows run a program from the maker at every start (anti-theft tools, driver installers).", u, a, risk="advanced", after="restart", src="WinUtil",
   more="Some laptops use this for anti-theft software (Absolute/Computrace). Desktops rarely need it.")
CS = r"HKCU\Software\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore"
for cap, label, what, warn in [("webcam", "Camera", "your camera", "Discord, Teams and games lose the camera."), ("microphone", "Microphone", "your microphone", "Discord, games and calls lose the microphone, also the phone mic in apps."),
                         ("contacts", "Contacts", "your contacts", None), ("phoneCallHistory", "Call history", "your call history", None), ("chat", "Messages", "your text messages", None), ("email", "Email", "your email", None),
                         ("userDataTasks", "Tasks", "your tasks", None), ("appointments", "Calendar", "your calendar", None), ("radios", "Radios", "Bluetooth and Wi-Fi on and off", None),
                         ("documentsLibrary", "Documents", "your Documents folder", None), ("picturesLibrary", "Pictures", "your Pictures folder", None), ("videosLibrary", "Videos", "your Videos folder", None),
                         ("broadFileSystemAccess", "All files", "every file on the PC", None), ("userNotificationListener", "Notifications", "your notifications", None), ("userAccountInformation", "Account info", "your name and account picture", None)]:
    sw(f"privacy.access.{cap}", "privacy", f"Apps can use: {label}", f"Store apps and apps that ask Windows may use {what}." + (f" Off: {warn}" if warn else ""),
       [reg(CS + "\\" + cap, "Value", "Allow", "string")], [reg(CS + "\\" + cap, "Value", "Deny", "string")], risk="behaviour", group="App permissions")

# ----------------------------------------------------------------------------- Start & search
w_off("DisableBing", "start.bing", "start", "Bing and Copilot in Start search", "Typing in Start also searches the web with Bing and offers Copilot. Off: Start searches this PC only.", rec="off", after="explorer")
w_off("DisableStartRecommended", "start.recommended", "start", "Recommended in Start", "Start's Recommended section: recent files, new apps, suggestions.", after="explorer")
sw("start.recent_apps", "start", "Recently added apps in Start", "Start lists apps you installed lately at the top.",
   [gone(POL + r"\Explorer", "HideRecentlyAddedApps")], [reg(POL + r"\Explorer", "HideRecentlyAddedApps", 1)], min=22000, after="explorer")
w_off("DisableStartPhoneLink", "start.phonelink", "start", "Phone Link in Start", "Start shows your phone's apps and messages beside it (Phone Link).", rec="off")
w_group("StartAllAppsView", "start.allapps", "start", "All apps in Start", "How Start lists every app.", [("hide", "Hidden"), ("category", "By category"), ("grid", "Grid"), ("list", "List")])
a, u = winutil("WPFTweaksRevertStartMenu")
sw("start.old_layout", "start", "Previous Start layout", "Brings back Start's layout from before the 2025 redesign, on builds that still have it.", a, u, default="off", risk="behaviour", after="explorer", src="WinUtil", min=26100, max=26200)
action("start.unpin_all", "start", "Unpin everything from Start", "Removes every pinned app from Start for you (it starts empty, you pin what you want). Undo puts your pins back.", "start_clear", min=22621, after="explorer", label="Unpin all")

# ----------------------------------------------------------------------------- Taskbar
a, u, mn, _ = w11d("TaskbarAlignLeft")
choice("taskbar.align", "taskbar", "Taskbar icons", "Where the taskbar's icons sit.", [("center", "Centre", u), ("left", "Left", a)], "center", min=mn)
w_group("SearchIcon", "taskbar.search", "taskbar", "Search on the taskbar", "The search box, a smaller button, or nothing (Win+S still searches).", [("box", "Box"), ("label", "Icon and label"), ("icon", "Icon"), ("hidden", "Hidden")])
w_off("HideTaskview", "taskbar.taskview", "taskbar", "Task view button", "The button for virtual desktops and open windows (Win+Tab does the same).")
sw("taskbar.widgets", "taskbar", "Widgets", "The weather and news board on the taskbar and lock screen.",
   [gone(r"HKLM\SOFTWARE\Policies\Microsoft\Dsh", "AllowNewsAndInterests")], [reg(r"HKLM\SOFTWARE\Policies\Microsoft\Dsh", "AllowNewsAndInterests", 0)], min=22000, rec="off", after="explorer")
w_off("HideChat", "taskbar.chat", "taskbar", "Chat and Meet now", "The Teams Chat and Meet now buttons of older Windows versions.", max=22621)
w_on("EnableEndTask", "taskbar.endtask", "taskbar", "End task on right-click", "Right-clicking an app on the taskbar offers \"End task\", which closes it even when it hangs.", rec="on")
w_on("EnableLastActiveClick", "taskbar.lastactive", "taskbar", "Click goes to the last window", "Clicking an app with several windows opens the one you used last instead of showing them all.")
w_group("CombineButtons", "taskbar.combine", "taskbar", "Combine buttons (main display)", "Whether an app's windows share one button, and whether names show.", [("always", "Always"), ("full", "When full"), ("never", "Never")], after="explorer")
w_group("CombineMMButtons", "taskbar.combine_mm", "taskbar", "Combine buttons (other displays)", "The same for the taskbars on your other monitors.", [("always", "Always"), ("full", "When full"), ("never", "Never")], after="explorer")
w_group("MultiMon", "taskbar.mm_mode", "taskbar", "Show apps on", "With several monitors: which taskbars show an app's button.", [("all", "All taskbars"), ("main_active", "Main and where it's open"), ("active", "Where it's open")], after="explorer")
a, u = winutil("WPFToggleBatteryPercentage")
sw("taskbar.battery", "taskbar", "Battery percentage", "Shows the battery level as a number next to its icon.", a, u, default="off", src="WinUtil", needs=["battery"])
a, u = winutil("WPFTweaksDisableNotifications")
sw("taskbar.notifications", "taskbar", "Notifications and calendar", "The notification centre and pop-ups from apps and Windows.", u, a, risk="advanced", after="signout", src="WinUtil",
   more="Off also hides the calendar flyout from the clock. Deck's own pop-ups are its own window and keep working.")
sw("taskbar.seconds", "taskbar", "Seconds in the clock", "The taskbar clock shows seconds.", [reg(ADV, "ShowSecondsInSystemClock", 1)], [reg(ADV, "ShowSecondsInSystemClock", 0)], default="off", min=22621)

# ----------------------------------------------------------------------------- File Explorer
w_on("ShowKnownFileExt", "explorer.ext", "explorer", "File extensions", "Shows .exe, .jpg, .txt after file names, so a \"photo.jpg.exe\" can't hide what it is.", rec="on", after="explorer")
w_on("ShowHiddenFolders", "explorer.hidden", "explorer", "Hidden files and folders", "Shows files and folders marked hidden (like AppData).", after="explorer")
sw("explorer.superhidden", "explorer", "Protected system files", "Shows Windows' own protected files. Deleting or moving them can break Windows.",
   [reg(ADV, "ShowSuperHidden", 1)], [reg(ADV, "ShowSuperHidden", 0)], default="off", risk="advanced", after="explorer")
w_group("ExplorerLocation", "explorer.launch", "explorer", "File Explorer opens to", "The folder a new Explorer window shows.", [("home", "Home"), ("thispc", "This PC"), ("downloads", "Downloads"), ("onedrive", "OneDrive")], after="explorer")
w_off("HideHome", "explorer.home", "explorer", "Home in the side pane", "Explorer's Home (recent and pinned files) in the left pane.", after="explorer")
w_off("HideGallery", "explorer.gallery", "explorer", "Gallery in the side pane", "Explorer's Gallery (your photos) in the left pane.", after="explorer")
w_off("HideOnedrive", "explorer.onedrive", "explorer", "OneDrive in the side pane", "OneDrive in Explorer's left pane (OneDrive keeps syncing).", after="explorer")
w_off("HideDupliDrive", "explorer.dup_drives", "explorer", "USB drives listed twice", "Windows lists each USB drive under This PC and again on its own. Off: once.", after="explorer")
w_on("AddFoldersToThisPC", "explorer.thispc_folders", "explorer", "Your folders under This PC", "Desktop, Documents, Downloads, Music, Pictures and Videos shown in This PC again.", after="explorer")
w_off("Hide3dObjects", "explorer.3dobjects", "explorer", "3D Objects under This PC", "The 3D Objects folder of Windows 10.", after="explorer")
w_off("HideMusic", "explorer.music", "explorer", "Music under This PC", "The Music folder in This PC (Windows 10).", after="explorer")
w_group("DriveLetterPosition", "explorer.letters", "explorer", "Drive letters", "Where drive letters show next to drive names.", [("after", "After the name"), ("before", "Before the name"), ("net_before", "Network drives first"), ("hidden", "Hidden")], after="explorer")
FT = r"HKCU\Software\Classes\Local Settings\Software\Microsoft\Windows\Shell\Bags\AllFolders\Shell"
sw("explorer.discovery", "explorer", "Folder type guessing", "Explorer looks at a folder's files to pick a view (Pictures, Music). In big folders that makes it slow to open.",
   [gone(FT, "FolderType")], [reg(FT, "FolderType", "NotSpecified", "string")], risk="behaviour", after="explorer", src="WinUtil (idea), Deck")
a, u = winutil("WPFToggleLongPaths")
sw("explorer.longpaths", "explorer", "Long paths", "Programs that support it may use paths longer than 260 characters.", a, u, default="off", risk="behaviour", src="WinUtil")
sw("explorer.compact", "explorer", "Compact view", "Less space between items in Explorer's lists.", [reg(ADV, "UseCompactMode", 1)], [reg(ADV, "UseCompactMode", 0)], default="off", after="explorer")
sw("explorer.fullpath", "explorer", "Full path in the title bar", "Explorer's title shows the whole folder path.",
   [reg(r"HKCU\Software\Microsoft\Windows\CurrentVersion\Explorer\CabinetState", "FullPath", 1)], [reg(r"HKCU\Software\Microsoft\Windows\CurrentVersion\Explorer\CabinetState", "FullPath", 0)], default="off", after="explorer")
EX = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Explorer"
sw("explorer.recent", "explorer", "Recent files and frequent folders", "Home and Quick access list the files and folders you used lately.",
   [reg(EX, "ShowRecent", 1), reg(EX, "ShowFrequent", 1)], [reg(EX, "ShowRecent", 0), reg(EX, "ShowFrequent", 0)], after="explorer")
sw("explorer.checkboxes", "explorer", "Check boxes to select files", "A tick box on each file for picking several without Ctrl.", [reg(ADV, "AutoCheckSelect", 1)], [reg(ADV, "AutoCheckSelect", 0)], default="off", after="explorer")

# ----------------------------------------------------------------------------- Right-click menu
w_on("RevertContextMenu", "context.classic", "context", "Classic right-click menu", "The full Windows 10 menu at once, without \"Show more options\".", risk="behaviour", after="explorer")
w_off("HideIncludeInLibrary", "context.library", "context", "\"Include in library\"", "The Include in library entry on folders.", after="explorer")
w_off("HideGiveAccessTo", "context.giveaccess", "context", "\"Give access to\"", "The Give access to (sharing) entry.", after="explorer")
w_off("HideShare", "context.share", "context", "\"Share\"", "The Share entry on files.", after="explorer")

# ----------------------------------------------------------------------------- Appearance
link("appearance.dark", "appearance", "Dark mode", "Deck's Display page switches dark and light mode, also on a schedule.", "system/display", "Open Display")
w_off("DisableTransparency", "appearance.transparency", "appearance", "Transparency effects", "See-through Start, taskbar and window backgrounds.")
sw("appearance.animations", "appearance", "Animations", "Windows zoom when they minimise and maximise, and the taskbar animates.",
   [reg(r"HKCU\Control Panel\Desktop\WindowMetrics", "MinAnimate", "1", "string"), reg(ADV, "TaskbarAnimations", 1)],
   [reg(r"HKCU\Control Panel\Desktop\WindowMetrics", "MinAnimate", "0", "string"), reg(ADV, "TaskbarAnimations", 0)], more="Off makes Windows feel snappier on a slow PC. Every effect one by one: Performance Options below.", src="Win11Debloat (idea), Deck")
link("appearance.visualfx", "appearance", "Visual effects (all of them)", "Windows' Performance Options, with every visual effect one by one.", "open:SystemPropertiesPerformance.exe", "Open Performance Options")
w_group("DesktopSpotlight", "appearance.spotlight", "appearance", "Windows Spotlight on the desktop", "A new desktop picture every day from Bing.", [("on", "On"), ("on_noicon", "On, without \"Learn about this picture\""), ("off", "Off")])
a, u = winutil("WPFToggleScrollbars")
sw("appearance.scrollbars", "appearance", "Scrollbars always visible", "Windows hides thin scrollbars until you hover. On: they always show.", a, u, default="off", src="WinUtil")
sw("appearance.titlebars", "appearance", "Accent colour on title bars", "Window title bars and borders use your accent colour.",
   [reg(r"HKCU\Software\Microsoft\Windows\DWM", "ColorPrevalence", 1)], [reg(r"HKCU\Software\Microsoft\Windows\DWM", "ColorPrevalence", 0)], default="off")
sw("appearance.logon_blur", "appearance", "Blur on the sign-in screen", "The sign-in screen blurs the lock screen picture behind it.",
   [gone(POL + r"\System", "DisableAcrylicBackgroundOnLogon")], [reg(POL + r"\System", "DisableAcrylicBackgroundOnLogon", 1)], src="WinUtil")

# ----------------------------------------------------------------------------- Multitasking
w_off("DisableWindowSnapping", "multitask.snap", "multitask", "Window snapping", "Dragging a window to an edge snaps it to half or a corner.", risk="behaviour")
w_off("DisableSnapAssist", "multitask.snapassist", "multitask", "Snap suggestions", "After snapping a window, Windows suggests what to put beside it.")
w_off("DisableSnapLayouts", "multitask.snaplayouts", "multitask", "Snap layouts", "The layout picker on the maximise button and at the top of the screen.")
w_group("ShowTabsInAltTab", "multitask.alttab", "multitask", "Browser tabs in Alt+Tab", "Edge tabs show in Alt+Tab like windows.", [("none", "None"), ("3", "3 recent"), ("5", "5 recent"), ("20", "20 recent")])
SK = r"HKCU\Control Panel\Accessibility\StickyKeys"
sw("multitask.stickykeys", "multitask", "Sticky Keys shortcut", "Pressing Shift five times turns on Sticky Keys (and asks in the middle of a game).",
   [{"reg": SK, "name": "Flags", "bits": 4, "set": True}], [{"reg": SK, "name": "Flags", "bits": 4, "set": False}], rec="off", src="Win11Debloat (idea), Deck")
KB = [r"HKU\.DEFAULT\Control Panel\Keyboard", r"HKCU\Control Panel\Keyboard"]
sw("multitask.numlock", "multitask", "Num Lock on at start", "The number pad works right away after start-up.",
   [{"reg": k, "name": "InitialKeyboardIndicators", "bits": 2, "set": True} for k in KB], [{"reg": k, "name": "InitialKeyboardIndicators", "bits": 2, "set": False} for k in KB],
   default="off", src="WinUtil (idea), Deck", after="restart")

# ----------------------------------------------------------------------------- Gaming
link("gaming.gamemode", "gaming", "Game Mode", "Deck's CPU profiles set Game Mode; change it there so the two don't fight.", "cpu", "Open CPU")
w_off("DisableDVR", "gaming.dvr", "gaming", "Game recording (Xbox Game DVR)", "Windows can record your games in the background, which costs some performance.", rec="off")
w_off("DisableGameBarIntegration", "gaming.gamebar", "gaming", "Game Bar", "Win+G and the controller's Xbox button open the Xbox Game Bar.", risk="behaviour",
      more="Off also stops the \"You'll need a new app to open this ms-gamebar link\" message after removing the Xbox apps.")
w_off("DisableMouseAcceleration", "gaming.mouseaccel", "gaming", "Mouse acceleration", "\"Enhance pointer precision\": the pointer goes further when you move faster. Off: the same hand movement always moves it the same distance.", risk="behaviour")
GC = r"HKCU\System\GameConfigStore"
sw("gaming.fso", "gaming", "Fullscreen optimisations", "Windows runs fullscreen games through its desktop compositor (alt-tab is faster, overlays work). Off brings back old exclusive fullscreen for every game.",
   [gone(GC, "GameDVR_FSEBehaviorMode"), gone(GC, "GameDVR_HonorUserFSEBehaviorMode"), gone(GC, "GameDVR_DXGIHonorFSEWindowsCompatible")],
   [reg(GC, "GameDVR_FSEBehaviorMode", 2), reg(GC, "GameDVR_HonorUserFSEBehaviorMode", 1), reg(GC, "GameDVR_DXGIHonorFSEWindowsCompatible", 1)], risk="advanced")
DX = r"HKCU\Software\Microsoft\DirectX\UserGpuPreferences"
choice("gaming.windowed", "gaming", "Optimisations for windowed games", "Lets DirectX 10 and 11 games in a window use the faster flip model (lower latency, VRR).",
       [("default", "Windows' choice", [{"dx": "SwapEffectUpgradeEnable", "value": None}]), ("on", "On", [{"dx": "SwapEffectUpgradeEnable", "value": "1"}]), ("off", "Off", [{"dx": "SwapEffectUpgradeEnable", "value": "0"}])], "default", risk="behaviour", min=22621)
choice("gaming.vrr", "gaming", "Variable refresh rate for windowed games", "G-Sync / FreeSync for games that don't support it themselves, in a window.",
       [("default", "Windows' choice", [{"dx": "VRROptimizeEnable", "value": None}]), ("on", "On", [{"dx": "VRROptimizeEnable", "value": "1"}]), ("off", "Off", [{"dx": "VRROptimizeEnable", "value": "0"}])], "default", risk="behaviour")
GD = r"HKLM\SYSTEM\CurrentControlSet\Control\GraphicsDrivers"
choice("gaming.hags", "gaming", "Hardware-accelerated GPU scheduling", "The GPU schedules its own work (can lower latency; needed for DLSS frame generation).",
       [("default", "Driver's choice", [gone(GD, "HwSchMode")]), ("on", "On", [reg(GD, "HwSchMode", 2)]), ("off", "Off", [reg(GD, "HwSchMode", 1)])], "default", risk="behaviour", after="restart")
sw("gaming.mpo", "gaming", "Multiplane overlay", "Lets the GPU show video and games as separate layers. Off fixes flicker and stutter on some NVIDIA setups, but can raise latency a little.",
   [gone(r"HKLM\SOFTWARE\Microsoft\Windows\Dwm", "OverlayTestMode")], [reg(r"HKLM\SOFTWARE\Microsoft\Windows\Dwm", "OverlayTestMode", 5)], risk="advanced", after="restart", src="WinUtil")

# ----------------------------------------------------------------------------- AI & Copilot
w_off("DisableCopilot", "ai.copilot", "ai", "Copilot", "Microsoft Copilot and its taskbar button.", rec="off", after="explorer")
a, u, mn, mx = w11d("DisableRecall")
sw("ai.recall", "ai", "Recall", "Recall saves pictures of your screen to search later (Copilot+ PCs).", [gone(s["reg"], s["name"]) for s in a if "name" in s], a, rec="off", after="restart", src="Win11Debloat", min=mn)
w_off("DisableClickToDo", "ai.clicktodo", "ai", "Click to Do", "Windows looks at what's on screen to offer AI actions on text and pictures.", rec="off")
w_off("DisableAISvcAutoStart", "ai.service", "ai", "AI service starts by itself", "Windows' AI service (WSAIFabricSvc) starts with Windows. Off: only when something needs it.", risk="behaviour", after="restart", needs=["service:WSAIFabricSvc"])
w_off("DisableEdgeAI", "ai.edge", "ai", "AI in Edge", "Copilot in Edge's sidebar and on pages, AI writing help and AI history search.", rec="off", more="These are Edge policies, so Edge then says \"Your browser is managed by your organization\": that line comes from this tweak and goes away with Undo.")
w_off("DisableChromeAI", "ai.chrome", "ai", "AI in Chrome", "Gemini, Help me write, AI history search and other AI features in Chrome.", needs=["app:chrome"], more="These are Chrome policies, so Chrome then says \"Managed by your organization\": that line comes from this tweak and goes away with Undo.")
w_off("DisablePaintAI", "ai.paint", "ai", "AI in Paint", "Cocreator, Image Creator, generative fill and erase in Paint.")
w_off("DisableNotepadAI", "ai.notepad", "ai", "AI in Notepad", "Rewrite, summarise and other AI features in Notepad.")

# ----------------------------------------------------------------------------- Updates & Store
w_off("DisableUpdateASAP", "updates.asap", "updates", "Get updates as soon as they're out", "Windows installs new features before they reach everyone.", rec="off")
w_off("PreventUpdateAutoReboot", "updates.autoreboot", "updates", "Restart by itself after updates", "Windows may restart to finish an update while you're signed in (it waits for you when off).", rec="off")
w_off("DisableDeliveryOptimization", "updates.delivery", "updates", "Share updates with other PCs", "Your PC uploads parts of updates to other PCs on the internet or your network.", rec="off")
DS = r"HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\DriverSearching"
sw("updates.drivers", "updates", "Drivers from Windows Update", "Windows Update installs drivers too, and can replace the GPU driver you chose.",
   [gone(POL + r"\WindowsUpdate", "ExcludeWUDriversInQualityUpdate"), reg(DS, "SearchOrderConfig", 1)], [reg(POL + r"\WindowsUpdate", "ExcludeWUDriversInQualityUpdate", 1), reg(DS, "SearchOrderConfig", 0)], risk="behaviour")
sw("updates.store_auto", "updates", "Store apps update by themselves", "Microsoft Store updates your apps in the background.",
   [gone(r"HKLM\SOFTWARE\Policies\Microsoft\WindowsStore", "AutoDownload")], [reg(r"HKLM\SOFTWARE\Policies\Microsoft\WindowsStore", "AutoDownload", 2)], risk="behaviour")
UX = r"HKLM\SOFTWARE\Microsoft\WindowsUpdate\UX\Settings"
choice("updates.pause_limit", "updates", "Longest update pause", "How far Settings → Windows Update lets you pause (it doesn't pause anything by itself).",
       [("default", "Windows' limit (5 weeks)", [gone(UX, "FlightSettingsMaxPauseDays")]), ("year", "1 year", [reg(UX, "FlightSettingsMaxPauseDays", 365)]), ("long", "10 years", [reg(UX, "FlightSettingsMaxPauseDays", 3650)])], "default", risk="advanced",
       more="A paused PC gets no security fixes either. Pause only as long as you need.")

# ----------------------------------------------------------------------------- Network
T6 = r"HKLM\SYSTEM\CurrentControlSet\Services\Tcpip6\Parameters"
sw("network.prefer_ipv4", "network", "Prefer IPv4 over IPv6", "Connections try IPv4 first. IPv6 keeps working.", [{"reg": T6, "name": "DisabledComponents", "bits": 0x20, "set": True}], [{"reg": T6, "name": "DisabledComponents", "bits": 0x20, "set": False}],
   default="off", risk="behaviour", after="restart", src="WinUtil")
TT = r"HKLM\SOFTWARE\Policies\Microsoft\Windows\TCPIP\v6Transition"
sw("network.teredo", "network", "Teredo", "An IPv6 tunnel over IPv4. Off can lower latency; Xbox party chat and some Game Pass multiplayer need it.",
   [gone(TT, "Teredo_State")], [reg(TT, "Teredo_State", "Disabled", "string")], risk="advanced", after="restart", src="WinUtil (idea), Deck")
choice("network.dns", "network", "DNS server", "Who turns names into addresses for this PC's connection. Automatic uses your router's.",
       [("auto", "Automatic", [{"dns": []}]), ("cloudflare", "Cloudflare", [{"dns": ["1.1.1.1", "1.0.0.1"]}]), ("google", "Google", [{"dns": ["8.8.8.8", "8.8.4.4"]}]),
        ("quad9", "Quad9", [{"dns": ["9.9.9.9", "149.112.112.112"]}]), ("adguard", "AdGuard (blocks ads)", [{"dns": ["94.140.14.14", "94.140.15.15"]}])], "auto", risk="behaviour", src="WinUtil (idea), Deck")
MM = r"HKLM\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Multimedia\SystemProfile"
sw("network.throttling", "network", "Network throttling for media", "While audio or video plays, Windows caps other network traffic a little. Off removes the cap.",
   [reg(MM, "NetworkThrottlingIndex", 10)], [reg(MM, "NetworkThrottlingIndex", 0xFFFFFFFF)], risk="advanced", after="restart")

# ----------------------------------------------------------------------------- System
sw("system.hibernate", "system", "Hibernation", "Saves memory to disk to power off completely. Off frees hiberfil.sys (as big as a good part of your RAM) and also turns off fast start-up.",
   [{"cmd": "hibernate", "on": True}], [{"cmd": "hibernate", "on": False}], risk="behaviour", src="WinUtil")
w_off("DisableFastStartup", "system.faststartup", "system", "Fast start-up", "Shut down saves part of Windows to disk to start faster. Off: real shutdowns (fixes dual boot and some driver problems).", risk="behaviour")
a, u = winutil("WPFToggleDetailedBSoD")
sw("system.bsod", "system", "Detailed blue screens", "Blue screens show the technical details (driver and stop code) instead of the sad face only.", a, u, default="off", src="WinUtil")
a, u = winutil("WPFToggleVerboseLogon")
sw("system.verbose", "system", "Detailed start-up messages", "Start-up, sign-in and shutdown say what Windows is doing (\"Applying settings…\").", a, u, default="off", src="WinUtil")
w_off("DisableStorageSense", "system.storagesense", "system", "Storage Sense", "Windows deletes temporary files and old Recycle Bin items by itself when space runs low.", risk="behaviour")
sw("system.reserved", "system", "Reserved storage", "Windows keeps 7-10 GB free for updates. Off gives it back; turn it on again before a big feature update.",
   [{"cmd": "reserved_storage", "on": True}], [{"cmd": "reserved_storage", "on": False}], risk="advanced", src="WinUtil")
w_off("DisableBitlockerAutoEncryption", "system.bitlocker_auto", "system", "Automatic device encryption", "New installs turn on BitLocker device encryption by themselves. Off stops that; it never decrypts a drive that is already encrypted.", risk="behaviour")
w_off("DisableModernStandbyNetworking", "system.standby_net", "system", "Network while asleep (Modern Standby)", "The laptop stays online while asleep for mail and updates.", risk="behaviour", needs=["battery"])
a, u = winutil("WPFTweaksUTC")
sw("system.utc", "system", "Hardware clock in UTC", "For dual booting with Linux, which keeps the clock in UTC: stops the time jumping by hours between them.", [dict(s, type="dword") if "type" in s else s for s in a], [dict(s, type="dword") if "type" in s else s for s in u], default="off", risk="advanced", after="restart", src="WinUtil")
w_off("DisableSettingsHome", "system.settings_home", "system", "Settings Home page", "Settings opens on Home (recommendations and ads). Off: it opens on System.")
a, u = winutil("WPFToggleDisableLockscreen")
sw("system.lockscreen", "system", "Lock screen", "The picture screen before sign-in. Off goes straight to the sign-in box.", u, a, risk="behaviour", src="WinUtil")
sw("system.sandbox", "system", "Windows Sandbox", "A throwaway Windows in a window for trying unknown programs (Pro, Enterprise).", [{"feature": "Containers-DisposableClientVM", "on": True}], [{"feature": "Containers-DisposableClientVM", "on": False}],
   default="off", risk="behaviour", after="restart", src="Win11Debloat", needs=["edition:pro"], min=18362)
sw("system.wsl", "system", "Windows Subsystem for Linux", "Run Linux tools and distributions on Windows.", [{"feature": "Microsoft-Windows-Subsystem-Linux", "on": True}, {"feature": "VirtualMachinePlatform", "on": True}],
   [{"feature": "Microsoft-Windows-Subsystem-Linux", "on": False}, {"feature": "VirtualMachinePlatform", "on": False}], default="off", risk="behaviour", after="restart", src="Win11Debloat", min=19041)

# ----------------------------------------------------------------------------- Advanced
HV = r"HKLM\SYSTEM\CurrentControlSet\Control\DeviceGuard\Scenarios\HypervisorEnforcedCodeIntegrity"
sw("advanced.memory_integrity", "advanced", "Memory integrity (core isolation)", "Windows checks every driver in a protected area, which stops kernel malware. Some old drivers and tools need it off; it can cost a few % in games.",
   [reg(HV, "Enabled", 1)], [reg(HV, "Enabled", 0)], default="on", risk="advanced", after="restart",
   more="FACEIT and some anti-cheats require it on, and Windows Security warns while it's off.")
sw("advanced.sysmain", "advanced", "SysMain (Superfetch)", "Preloads programs you use often into memory. Helps hard drives; on an SSD it mostly writes to the disk.",
   [{"service": "SysMain", "start": "auto"}], [{"service": "SysMain", "start": "disabled"}], risk="advanced", needs=["system_hdd"])
sw("advanced.search_service", "advanced", "Windows Search indexing", "Indexes your files so Start and Explorer find them fast. Off saves disk work; searches get slow (Deck's own search keeps working).",
   [{"service": "WSearch", "start": "auto"}], [{"service": "WSearch", "start": "disabled"}], risk="advanced")
a, u = winutil("WPFTweaksDisableBGapps")
sw("advanced.bgapps", "advanced", "Store apps in the background", "Store apps may run in the background (for notifications and live tiles).", u, a, risk="advanced", src="WinUtil")
a, u = winutil("WPFTweaksDisableWarningForUnsignedRdp")
sw("advanced.rdp_warning", "advanced", "Warnings for unsigned RDP files", "Windows warns before opening a Remote Desktop file nobody signed.", u, a, risk="advanced", src="WinUtil")
a, u = winutil("WPFTweaksBraveDebloat")
sw("advanced.brave", "advanced", "Brave extras", "Brave Rewards, Wallet, VPN, Leo AI, News, Talk and Tor windows.", u, a, src="WinUtil", needs=["app:brave"], rec="off")
a, u = winutil("WPFTweaksEdgeDebloat")
sw("advanced.edge", "advanced", "Edge extras and data sharing", "Edge's first-run pages, shopping, rewards, collections, desktop shortcut and data sharing. Edge and WebView2 stay installed.", u, a, src="WinUtil", rec="off", more="These are Edge policies, so Edge then says \"Your browser is managed by your organization\": that line comes from this tweak and goes away with Undo.")

# ----------------------------------------------------------------------------- presets
PRESETS = [
    {"id": "privacy", "name": "Privacy", "what": "Send Microsoft and apps as little as Windows allows.",
     "set": {"privacy": "off", "privacy.activity": "off", "privacy.feedback": "off", "privacy.cloud_search": "off", "privacy.consumer": "off", "privacy.edge_ads": "off", "privacy.device_apps": "off", "start.bing": "off", "ai.recall": "off", "updates.delivery": "off"}},
    {"id": "clean", "name": "Clean Start & taskbar", "what": "No ads, Bing, widgets or Copilot in Start and on the taskbar.",
     "set": {"start.bing": "off", "start.recommended": "off", "start.phonelink": "off", "taskbar.widgets": "off", "taskbar.taskview": "off", "taskbar.chat": "off", "ai.copilot": "off", "taskbar.endtask": "on", "explorer.ext": "on"}},
    {"id": "gaming", "name": "Gaming", "what": "Fewer interruptions and background recording while you play.",
     "set": {"gaming.dvr": "off", "gaming.mouseaccel": "off", "multitask.stickykeys": "off", "updates.autoreboot": "off", "updates.asap": "off", "privacy.feedback": "off"}},
    {"id": "quiet", "name": "Quiet Windows", "what": "No tips, nagging, suggestions or surprise restarts.",
     "set": {"privacy.suggestions": "off", "privacy.lockscreen_tips": "off", "privacy.settings_ads": "off", "privacy.feedback": "off", "privacy.consumer": "off", "updates.autoreboot": "off", "system.settings_home": "off"}},
]

CATS = [("privacy", "Privacy", "shield"), ("start", "Start & search", "search"), ("taskbar", "Taskbar", "layout"), ("explorer", "File Explorer", "folder"),
        ("context", "Right-click menu", "list"), ("appearance", "Appearance", "droplet"), ("multitask", "Multitasking", "windows"), ("gaming", "Gaming", "gamepad"),
        ("ai", "AI & Copilot", "sparkle"), ("updates", "Updates & Store", "download"), ("network", "Network", "wifi"), ("system", "System", "gear"), ("advanced", "Advanced", "alert")]


# ----------------------------------------------------------------------------- checks and output
def admin_needed(t):
    for o in t["options"]:
        for s in o["steps"]:
            k = s.get("reg", "")
            if k.startswith(("HKLM", "HKCR", "HKU")) or "\\Policies\\" in k or any(x in s for x in ("service", "feature", "cmd", "dns")):
                return True
    return t["kind"] == "action"


def main():
    ids, claimed, problems = set(), {}, []
    for t in TWEAKS:
        if t["id"] in ids:
            problems.append(f"duplicate id {t['id']}")
        ids.add(t["id"])
        if t["kind"] in ("switch", "choice"):
            if not all(o["steps"] for o in t["options"]):
                problems.append(f"{t['id']}: an option has no steps")
            if t["default"] not in [o["id"] for o in t["options"]]:
                problems.append(f"{t['id']}: default {t['default']} isn't an option")
        t["admin"] = admin_needed(t)
        seen = set()
        for o in t["options"]:
            for s in o["steps"]:
                if "reg" in s and "name" in s:
                    k = (s["reg"].lower().replace("hkey_current_user", "hkcu"), s["name"].lower())
                    seen.add(k)
        for k in seen:
            if k in claimed and claimed[k] != t["id"]:
                # the earlier tweak keeps the value: drop it from this one
                for o in t["options"]:
                    o["steps"] = [s for s in o["steps"] if not ("reg" in s and "name" in s and (s["reg"].lower().replace("hkey_current_user", "hkcu"), s["name"].lower()) == k)]
                print(f"  {t['id']}: {k[0]}\\{k[1]} belongs to {claimed[k]}, left out here")
            else:
                claimed[k] = t["id"]
    for p in PRESETS:
        for tid, opt in p["set"].items():
            t = next((x for x in TWEAKS if x["id"] == tid), None)
            if not t or opt not in [o["id"] for o in t["options"]]:
                problems.append(f"preset {p['id']}: {tid}={opt} isn't a tweak option")
    for t in TWEAKS:
        if t["rec"] and t["rec"] not in [o["id"] for o in t["options"]]:
            problems.append(f"{t['id']}: rec {t['rec']} isn't an option")
        if t["cat"] not in [c for c, _, _ in CATS]:
            problems.append(f"{t['id']}: unknown category {t['cat']}")
    if problems:
        raise SystemExit("\n".join(problems))
    os.makedirs(OUT, exist_ok=True)
    data = {"source": {"Win11Debloat": W11D_SHA, "WinUtil": WINUTIL_SHA}, "cats": [{"id": c, "name": n, "icon": i} for c, n, i in CATS], "presets": PRESETS, "tweaks": TWEAKS}
    json.dump(data, open(os.path.join(OUT, "tweaks.json"), "w", encoding="utf-8", newline="\n"), indent=1, ensure_ascii=False)
    # built-in apps: Win11Debloat's list without Edge (Deck runs on WebView2)
    apps = []
    for a in APPS["Apps"]:
        ids_ = a["AppId"] if isinstance(a["AppId"], list) else [a["AppId"]]
        if any("Edge" in i for i in ids_):
            continue
        apps.append({"name": a["FriendlyName"], "ids": ids_, "what": a.get("Description") or "", "rating": a["Recommendation"], "method": a["RemovalMethod"]})
    # Win11Debloat's empty Start layout, for "Unpin everything from Start"
    open(os.path.join(OUT, "start2.bin"), "wb").write(w11d_file(W11D_FILES["assets/start/start2.bin"]))
    json.dump({"source": W11D_SHA, "apps": apps}, open(os.path.join(OUT, "apps.json"), "w", encoding="utf-8", newline="\n"), indent=1, ensure_ascii=False)
    print(f"{len(TWEAKS)} tweaks, {len(PRESETS)} presets, {len(apps)} apps")


if __name__ == "__main__":
    main()
