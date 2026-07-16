import { describe, expect, it } from "vitest";

import { firstRunStageIndex } from "./jobs";

describe("firstRunStageIndex", () => {
  it("keeps the video pipeline journey monotonic through OCR and transcript writes", () => {
    const stages = [
      "fetching",
      "sampling_frames",
      "transcribing",
      "chunking_transcript",
      "writing_transcript_first",
      "transcript_indexed",
      "ocr_frames",
      "writing_transcript",
      "writing_index",
      "embedding_units",
      "embedding_unit_images",
      "completed",
    ];
    const indexes = stages.map((stage) =>
      firstRunStageIndex({ status: stage === "completed" ? "completed" : "running", stage }),
    );

    expect(indexes).toEqual([0, 0, 1, 1, 2, 2, 2, 2, 4, 4, 4, 5]);
    expect(indexes.every((value, index) => index === 0 || value >= indexes[index - 1])).toBe(
      true,
    );
  });

  it("reserves the fourth step for video understanding", () => {
    expect(firstRunStageIndex({ status: "running", stage: "analyzing_understanding" })).toBe(3);
  });
});
