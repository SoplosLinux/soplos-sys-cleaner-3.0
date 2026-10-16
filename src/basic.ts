import "./styles/global.css";
import BasicApp from "./BasicApp.svelte";
import { i18nReady } from "./i18n";

// Same reasoning as main.ts: no WebKitGTK context menu in the release build.
if (!import.meta.env.DEV) {
  document.addEventListener("contextmenu", (e) => e.preventDefault());
}

// svelte-i18n needs the initial locale's dictionary loaded before any
// component using $_() first renders.
i18nReady.then(() => {
  new BasicApp({
    target: document.getElementById("app")!,
  });
});
