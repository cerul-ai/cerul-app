import { BrandMark } from "./brand";

export function OverlayMark() {
  return <BrandMark className="overlay-mark" />;
}

export function OverlayWatermark() {
  return <BrandMark className="overlay-watermark" />;
}

export function OverlayHint({
  state,
  hotkeyLabel,
}: {
  state: "empty" | "loading" | "error" | "results" | "noresult";
  hotkeyLabel: string;
}) {
  if (state === "results" || state === "loading") {
    return (
      <span className="overlay-hint">
        <kbd>↑↓</kbd>
        <kbd>↵</kbd>
      </span>
    );
  }
  if (state === "noresult" || state === "error") {
    return (
      <span className="overlay-hint">
        <kbd>esc</kbd>
      </span>
    );
  }
  return (
    <span className="overlay-hint">
      <kbd>{hotkeyLabel}</kbd>
    </span>
  );
}

export function OverlayThumbGlyph({ contentType, chunkType }: { contentType: string; chunkType: string }) {
  if (contentType === "audio") {
    return (
      <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={2}>
        <path d="M4 9.5v5h3.5L12 18.5v-13L7.5 9.5H4Z" />
        <path d="M15.5 9a3.5 3.5 0 0 1 0 6" />
      </svg>
    );
  }
  if (contentType === "image" || isVisualChunk(chunkType)) {
    return (
      <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={2}>
        <rect x="4" y="5" width="16" height="14" rx="2" />
        <path d="m7 16 4-4 3 3 2-2 3 3" />
        <circle cx="9" cy="9" r="1" />
      </svg>
    );
  }
  return (
    <svg width="13" height="13" viewBox="0 0 24 24" fill="currentColor">
      <path d="M7 4.5v15l13-7.5z" />
    </svg>
  );
}

export function OverlayLoading() {
  return (
    <div className="overlay-skeleton">
      {[0, 1, 2].map((index) => (
        <div key={index} className="overlay-skeleton__row">
          <span className="overlay-skeleton__bar" style={{ width: 66, height: 40 }} />
          <span style={{ flex: 1 }}>
            <span
              className="overlay-skeleton__bar"
              style={{ display: "block", width: "58%", height: 12 }}
            />
            <span
              className="overlay-skeleton__bar"
              style={{ display: "block", width: "82%", height: 10, marginTop: 7 }}
            />
          </span>
          <span className="overlay-skeleton__bar" style={{ width: 34, height: 12 }} />
        </div>
      ))}
    </div>
  );
}

export function highlightOverlay(text: string, phrase: string) {
  const needle = phrase.trim();
  const index = needle ? text.toLowerCase().indexOf(needle.toLowerCase()) : -1;

  if (index === -1) {
    return text;
  }

  return (
    <>
      {text.slice(0, index)}
      <mark>{text.slice(index, index + needle.length)}</mark>
      {text.slice(index + needle.length)}
    </>
  );
}

function isVisualChunk(chunkType: string) {
  return chunkType === "keyframe" || chunkType === "image" || chunkType === "ocr" || chunkType === "understanding";
}
