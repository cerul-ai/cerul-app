import { useEffect, useState } from "react";

type SplashPhase = "playing" | "leaving" | "hidden";

export function LaunchSplash() {
  const [phase, setPhase] = useState<SplashPhase>("playing");

  useEffect(() => {
    const params = new URLSearchParams(window.location.hash.split("?")[1] ?? "");
    if (params.get("skipSplash") === "1") {
      setPhase("hidden");
      return;
    }
    const leaving = window.setTimeout(() => setPhase("leaving"), 1760);
    const hidden = window.setTimeout(() => setPhase("hidden"), 2280);
    return () => {
      window.clearTimeout(leaving);
      window.clearTimeout(hidden);
    };
  }, []);

  if (phase === "hidden") return null;

  return (
    <section
      className={`cerul-launch splash-playing${phase === "leaving" ? " is-leaving" : ""}`}
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
