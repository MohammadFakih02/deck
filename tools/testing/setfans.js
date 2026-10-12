// node cdp.mjs eval "$(sed s/MODE/\"curve\"/ setfans.js)"   (MODE = "auto" | "fixed" | "curve")
(async()=>{const s=await invoke("hw_state");const c=s.cfg;c.fans.channels.forEach(ch=>ch.mode=MODE);await invoke("hw_apply",{cfg:c});return "set "+MODE})()
