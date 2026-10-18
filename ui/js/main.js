// Deck's window: every page module (each registers itself in Pages), then the start. The modules
// import what they use from each other; this order only decides which file runs its own set-up first.
import { T, S, Pages, h, I, invoke, confirmBox, modal, setSetting } from "./core.js";
import "./layout.js";
import "./home.js";
import "./search.js";
import "./pages1.js";
import "./pages2.js";
import "./pages3.js";
import "./boost.js";
import "./curve.js";
import "./hw.js";
import { ovStyleNow, ovRefresh } from "./overlay.js";
import { editCpu, editGpu } from "./profiles.js";
import "./pcs.js";
import { boot, go, render, tick } from "./app.js";

// The UI tests (tools/testing/ui-preview) and DevTools reach these by name.
Object.assign(window, { T, S, Pages, h, I, invoke, confirmBox, modal, setSetting, ovStyleNow, ovRefresh, editCpu, editGpu, go, render, tick });

boot();
