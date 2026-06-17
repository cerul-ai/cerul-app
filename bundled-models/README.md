# Bundled Models

Small local runtime assets that are shipped inside the Cerul installer.

Current bundled OCR snapshots:

- `PaddlePaddle/PP-OCRv6_small_det_onnx`
  - Revision: `4fda2ea33fb340a1a19592aec4604ba1d2d5587d`
  - Files: `inference.onnx`, `inference.yml`, `README.md`
- `PaddlePaddle/PP-OCRv6_small_rec_onnx`
  - Revision: `2f0724790c8b57946c89cc45d2fa79e405781f51`
  - Files: `inference.onnx`, `inference.yml`, `README.md`

The sidecar uses the Hugging Face cache directory layout here so the same
snapshot validation logic works for bundled, mirrored, and downloaded models.
