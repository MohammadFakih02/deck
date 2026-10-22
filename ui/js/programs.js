// Maintenance → Apps: every installed program, with Windows' plain uninstall and an advanced one
// that also finds what the program left behind. Backend: src-tauri/src/programs.rs.
import { I, Pages, S, appIcon, confirmBox, fmtBytes, h, invoke, modal, note, seg } from "./core.js";
import { head } from "./pages1.js";
import { render, go } from "./app.js";
import { appsView, storeView } from "./tweaks.js";

const st = () => S.cache.progs;
const busy = () => S.cache.progBusy || {};

async function reload(force) {
  try { S.cache.progs = await invoke("programs_list", { force: !!force }); } catch (e) { note(String(e), true); }
  render();
}

function setBusy(id, text) {
  S.cache.progBusy = { ...busy(), [id]: text };
  if (!text) delete S.cache.progBusy[id];
  render();
}

// ------------------------------------------------------------------------------------- uninstalling

async function plain(p) {
  if (!(await confirmBox(`Uninstall ${p.name}?`, p.store ? "It's removed for your account." : `${p.name}'s own uninstaller opens. Answer its questions to finish. Afterwards Deck looks for what it left behind and lists it, so you choose what to keep and what to delete.`, "Uninstall", true))) return;
  setBusy(p.id, "Uninstalling…");
  try {
    const r = await invoke("programs_do", { what: "uninstall", id: p.id });
    if (r.gone && !p.store) {
      // every uninstall ends with what it left behind, for the user to pick from (nothing goes unasked)
      setBusy(p.id, "Looking for leftovers…");
      const s = await invoke("programs_do", { what: "scan" });
      setBusy(p.id, null);
      await leftovers(p, s.leftovers, r.detail || `${p.name}'s uninstaller finished and ${p.name} is gone from the list.`);
    } else if (r.detail) explain(r.gone ? `${p.name} uninstalled` : `${p.name} is still installed`, r.detail);
    // Windows Installer's own window just closes when it's done, so the outcome always gets a box
    else if (r.gone) explain(`${p.name} uninstalled`, `${p.name} was removed for your account.`);
    else explain(`${p.name} is still installed`, `${p.name}'s uninstaller closed without removing it (maybe it was cancelled). Try again, or use Advanced to remove what's left.`);
  } catch (e) { explain(`${p.name} wasn't uninstalled`, String(e)); }
  setBusy(p.id, null);
  reload(true);
}

// a result too long or important for a toast: what happened and why, in full
function explain(title, text) {
  return new Promise((done) => modal(title, h("p", { class: "dim", style: { margin: 0, userSelect: "text", wordBreak: "break-word" } }, text),
    (close) => [h("button", { class: "btn", onclick: () => navigator.clipboard?.writeText(text) }, "Copy"), h("button", { class: "btn", onclick: () => { close(); done(); } }, "OK")],
    // kept when Deck hides: the result must still be there when the user comes back
    { keep: true, onDismiss: done }));
}

async function advanced(p) {
  const ok = await new Promise((res) => modal(`Advanced uninstall: ${p.name}`, h("div", { class: "col", style: { gap: "8px" } },
    h("p", { class: "dim", style: { margin: 0 } }, p.can_uninstall
      ? `First ${p.name}'s own uninstaller runs. Then Deck looks for what it left behind: folders, settings in the registry and shortcuts.`
      : `${p.name}'s uninstaller is missing, so Deck goes straight to what's left of it: its entry in the list, folders, registry settings and shortcuts.`),
    h("p", { class: "faint small", style: { margin: 0 } }, "You pick what goes. Folders and files go to the Recycle Bin, and registry entries are saved to a backup file first.")),
  (close) => [h("button", { class: "btn", onclick: () => { close(); res(false); } }, "Cancel"), h("button", { class: "btn danger", onclick: () => { close(); res(true); } }, p.can_uninstall ? "Uninstall" : "Continue")]));
  if (!ok) return;
  let said = "";
  try {
    if (p.can_uninstall) {
      setBusy(p.id, "Uninstalling…");
      let r;
      try { r = await invoke("programs_do", { what: "uninstall_advanced", id: p.id }); } catch (e) { r = { gone: false, detail: String(e) }; }
      said = r.gone ? (r.detail || `${p.name}'s uninstaller finished and ${p.name} is gone from the list.`) : "";
      if (!r.gone &&!(await confirmBox(`${p.name} is still installed`, `${r.detail || "Its uninstaller closed without removing it (maybe it was cancelled)."} Look for leftovers anyway?`, "Look anyway"))) {
        setBusy(p.id, null);
        return reload(true);
      }
    } else {
      await invoke("programs_do", { what: "forget", id: p.id });
    }
    setBusy(p.id, "Looking for leftovers…");
    const r = await invoke("programs_do", { what: "scan" });
    setBusy(p.id, null);
    await leftovers(p, r.leftovers, said);
  } catch (e) { explain(`Advanced uninstall of ${p.name} stopped`, String(e)); setBusy(p.id, null); }
  reload(true);
}

function leftovers(p, items, said) {
  if (!items.length) {
    invoke("programs_do", { what: "drop_unfinished" });
    return explain(`${p.name} is uninstalled`, `${said ? said + " " : ""}Deck then looked for what it left behind (folders named after it, Start menu entries and shortcuts, registry settings, services) and found nothing, so there's nothing more to remove.`);
  }
  const picked = new Set(items.filter((x) => x.sure).map((x) => x.id));
  return new Promise((done) => {
    const groups = [["folder", "Folders"], ["file", "Shortcuts"], ["key", "Registry"], ["service", "Windows services (stopped and removed; a backup is saved)"]];
    const body = h("div", { class: "col prog-left" },
      said ? h("p", { class: "dim", style: { margin: 0 } }, said) : null,
      h("p", { class: "dim", style: { margin: 0 } }, `Found ${items.length} thing${items.length > 1 ? "s" : ""} ${p.name} left behind. Untick anything you want to keep.`),
      groups.map(([k, title]) => {
        const part = items.filter((x) => x.kind === k);
        return part.length ? [h("div", { class: "prog-left-h" }, title), part.map((x) => h("label", { class: "prog-left-i" },
          h("input", { type: "checkbox", checked: picked.has(x.id), onchange: (e) => e.target.checked ? picked.add(x.id) : picked.delete(x.id) }),
          h("div", { class: "grow", style: { minWidth: 0 } }, h("div", { class: "prog-left-p" }, x.path), h("div", { class: "faint small" }, x.why + (x.size != null ? ` · ${fmtBytes(x.size)}` : "")))))] : null;
      }));
    modal(`What ${p.name} left behind`, body, (close) => [
      h("button", { class: "btn", onclick: () => { close(); invoke("programs_do", { what: "drop_unfinished" }); done(); } }, "Keep all"),
      h("button", { class: "btn danger", onclick: async (e) => {
        if (!picked.size) { close(); return done(); }
        e.target.disabled = true; e.target.textContent = "Removing…";
        try {
          const r = await invoke("programs_do", { what: "remove", ids: [...picked] });
          note(r.failed.length ? `Removed ${picked.size - r.failed.length}; left ${r.failed.length}: ${r.failed[0]}` : `Removed ${picked.size} leftover${picked.size > 1 ? "s" : ""} of ${p.name}`, !!r.failed.length);
        } catch (err) { note(String(err), true); }
        close(); done();
      } }, "Remove selected"),
    ], { onDismiss: done, wide: true, keep: true });
  });
}

// ------------------------------------------------------------------------------------- the page

function row(p) {
  const b = busy()[p.id];
  const bits = [p.publisher, p.version, p.date, p.size ? fmtBytes(p.size) : null].filter(Boolean).join(" · ");
  return h("div", { class: "win prog" + (b ? " off" : ""), style: { gridTemplateColumns: "30px 1fr auto" } },
    appIcon(p.store ? "app" : "file", p.icon, p.name),
    h("div", { style: { minWidth: 0 } },
      h("div", { class: "t ell" }, p.name, p.store ? h("span", { class: "chip", style: { marginLeft: "6px" } }, "Store") : null,
        p.broken ? h("span", { class: "chip warn", style: { marginLeft: "6px" }, title: "Its uninstaller is gone. Advanced removes what's left." }, "Uninstaller missing") : null,
        p.unlisted ? h("span", { class: "chip", style: { marginLeft: "6px" }, title: "Windows Installer knows this app, but it's missing from Windows' list of programs (Settings doesn't show it). An old cleanup tool or a half-finished uninstall usually causes that. It uninstalls normally from here." }, "Missing from Windows' list") : null,
        p.files_gone ? h("span", { class: "chip warn", style: { marginLeft: "6px" }, title: "Its install folder is gone: it was half removed. Uninstall finishes it; Advanced also clears what's left." }, "Files gone") : null),
      h("div", { class: "s ell", title: p.location || "" }, bits || (p.location || ""))),
    b ? h("span", { class: "faint small" }, b) : h("div", { class: "row", style: { gap: "6px" } },
      p.location && !p.store ? h("button", { class: "btn sm ghost icon", title: "Open its folder", "aria-label": `Open ${p.name}'s folder`, onclick: () => invoke("open_path", { path: p.location, keep: true }).catch((e) => note(String(e), true)) }, I("folder", "sm")) : null,
      p.can_uninstall ? h("button", { class: "btn sm", onclick: () => plain(p) }, "Uninstall") : null,
      !p.store ? h("button", { class: "btn sm ghost", title: "Uninstall, then remove the folders, registry settings and shortcuts it leaves behind", onclick: () => advanced(p) }, I("broom", "sm"), "Advanced") : null));
}

const TABS = [["installed", "Installed"], ["builtin", "Built-in apps"], ["store", "Store apps & updates"]];
const tab = () => (TABS.some(([id]) => id === S.sub) ? S.sub : "installed");

Pages.programs = {
  async load() {
    if (tab() === "installed" && !st()) await reload(false);
  },
  render() {
    const t = tab();
    const tabs = seg(TABS, t, (v) => go("programs", v === "installed" ? null : v));
    if (t !== "installed") {
      const [title, sub] = t === "builtin"
        ? ["Built-in apps", "Apps that come with Windows. Remove the ones you don't use; each can be put back."]
        : ["Store apps & updates", "Update your Store apps, and install Store apps without the Microsoft Store."];
      return h("div", { class: "page-narrow tw" }, head("Apps", null, tabs), h("h2", { class: "tw-h2", style: { marginTop: "4px" } }, title), h("p", { class: "dim", style: { marginTop: 0 } }, sub),
        t === "builtin" ? appsView() : storeView());
    }
    if (!st() && !S.cache.progLoading) { S.cache.progLoading = true; reload(false).finally(() => (S.cache.progLoading = false)); }
    const x = st();
    const q = (S.cache.progQ || "").trim().toLowerCase();
    const kind = S.cache.progKind || "all", sort = S.cache.progSort || "name";
    const needs = (p) => p.unlisted || p.files_gone || p.broken;
    let list = (x?.programs || []).filter((p) => (kind === "all" || (kind === "needs" ? needs(p) : (kind === "store") === p.store))
      && (!q || (p.name + " " + p.publisher).toLowerCase().includes(q)));
    if (sort === "size") list = [...list].sort((a, b) => (b.size || 0) - (a.size || 0));
    if (sort === "date") list = [...list].sort((a, b) => (b.date || "").localeCompare(a.date || ""));
    const total = list.reduce((n, p) => n + (p.size || 0), 0);
    return h("div", { class: "page-narrow" },
      head("Apps", null, tabs),
      h("p", { class: "dim", style: { marginTop: 0 } }, "Everything installed on this PC. Uninstall as Windows does, or use Advanced to also clear what an app leaves behind."),
      h("div", { class: "row wrap tw-filters" },
        h("input", { type: "search", class: "grow", placeholder: "Search your apps…", "aria-label": "Search apps", value: S.cache.progQ || "",
          oninput: (e) => { S.cache.progQ = e.target.value; render(); const i = document.querySelector(".tw-filters input"); if (i) { i.focus(); i.setSelectionRange(i.value.length, i.value.length); } } }),
        seg([["all", "All"], ["desktop", "Desktop"], ["store", "Store"], ["needs", `Needs attention${(x?.programs || []).filter(needs).length ? " (" + (x?.programs || []).filter(needs).length + ")" : ""}`]], kind, (v) => { S.cache.progKind = v; render(); }),
        seg([["name", "Name"], ["size", "Size"], ["date", "Newest"]], sort, (v) => { S.cache.progSort = v; render(); }),
        h("button", { class: "btn sm ghost", onclick: () => { S.cache.progs = null; render(); reload(true); } }, I("refresh", "sm"), "Refresh")),
      !x ? h("div", { class: "empty" }, "Reading your apps…") : [
        x.unfinished ? h("div", { class: "tw-note warn", role: "alert" }, I("alert", "sm"),
          h("span", { class: "grow" }, `Deck was closed during the advanced uninstall of ${x.unfinished.name}, before it could look for what was left behind.`),
          h("button", { class: "btn sm primary", onclick: async () => {
            try { const r = await invoke("programs_do", { what: "scan" }); await leftovers({ name: x.unfinished.name }, r.leftovers); } catch (e) { note(String(e), true); }
            reload(true);
          } }, I("broom", "sm"), "Look for leftovers now"),
          h("button", { class: "btn sm ghost", onclick: async () => { await invoke("programs_do", { what: "drop_unfinished" }); reload(true); } }, "Forget it")) : null,
        x.store_failed ? h("div", { class: "tw-note warn", role: "alert" }, I("alert", "sm"), h("span", { class: "grow" }, "Windows didn't list your Store apps this time, so they're missing below (they're still installed). Press Refresh to try again.")) : null,
        h("div", { class: "faint small", style: { margin: "4px 0 10px" } }, `${list.length} app${list.length === 1 ? "" : "s"}` + (total ? ` · ${fmtBytes(total)} that report their size` : "")),
        list.length ? h("div", { class: "col", style: { gap: "8px" } }, list.map(row)) : h("div", { class: "empty" }, q ? "No app matches." : "Nothing here."),
        !x.admin ? h("p", { class: "faint small", style: { marginTop: "12px" } }, "Without admin rights, apps installed for all users may ask for permission when you uninstall them.") : null,
      ]);
  },
};
