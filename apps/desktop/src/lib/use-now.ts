// A once-per-second clock for live elapsed/ETA readouts. Data polls only every
// few seconds, so without this the elapsed timer would visibly stall between
// refreshes. Pass `active = false` to stop the interval when nothing is running.

import { useEffect, useState } from "react";

export function useNowSeconds(active: boolean): number {
  const [now, setNow] = useState(() => Date.now() / 1000);
  useEffect(() => {
    if (!active) {
      return;
    }
    const id = window.setInterval(() => setNow(Date.now() / 1000), 1000);
    return () => window.clearInterval(id);
  }, [active]);
  return now;
}
