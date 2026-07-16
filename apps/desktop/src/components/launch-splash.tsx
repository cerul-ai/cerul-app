import { useEffect, useState } from "react";

type SplashPhase = "waiting" | "playing" | "leaving" | "hidden";

const SPLASH_LEAVE_AFTER_MS = 2760;
const SPLASH_HIDE_AFTER_MS = 3320;

export function LaunchSplash() {
  // Packaged builds can create the renderer while the main window is hidden
  // (notably Start at login, which launches Electron with `--hidden`). Do not
  // spend the animation timers in the background; play on the first reveal.
  const [phase, setPhase] = useState<SplashPhase>("waiting");

  useEffect(() => {
    const params = new URLSearchParams(window.location.hash.split("?")[1] ?? "");
    const forceMotion = import.meta.env.DEV && params.get("forceMotion") === "1";
    const reducedMotion = !forceMotion && window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    if (params.get("skipSplash") === "1" || reducedMotion) {
      setPhase("hidden");
      return;
    }

    let leaving: number | undefined;
    let hidden: number | undefined;
    const playWhenVisible = () => {
      if (document.visibilityState !== "visible") return;
      document.removeEventListener("visibilitychange", playWhenVisible);
      setPhase("playing");
      leaving = window.setTimeout(() => setPhase("leaving"), SPLASH_LEAVE_AFTER_MS);
      hidden = window.setTimeout(() => setPhase("hidden"), SPLASH_HIDE_AFTER_MS);
    };

    document.addEventListener("visibilitychange", playWhenVisible);
    playWhenVisible();
    return () => {
      document.removeEventListener("visibilitychange", playWhenVisible);
      if (leaving !== undefined) window.clearTimeout(leaving);
      if (hidden !== undefined) window.clearTimeout(hidden);
    };
  }, []);

  if (phase === "hidden") return null;

  return (
    <section
      className={`cerul-launch${phase === "waiting" ? " is-waiting" : " splash-playing"}${phase === "leaving" ? " is-leaving" : ""}`}
      aria-hidden="true"
    >
      <div className="cerul-launch__inner">
        <img className="cerul-launch__mark" src="/brand/svg/cerul-icon-paper.svg" alt="" />
        <span className="cerul-launch__rule" />
        <strong className="cerul-launch__word">Cerul</strong>
        <span className="cerul-launch__tagline">Where video becomes citable</span>
      </div>
    </section>
  );
}
