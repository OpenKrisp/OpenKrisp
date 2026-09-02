# Pinned model asset

`DeepFilterNet3_ll_onnx.tar.gz` is the official low-latency DeepFilterNet3 ONNX model from upstream
tag `v0.5.6`, commit `978576aa8400552a4ce9730838c635aa30db5e61`.

- Source: <https://github.com/Rikorose/DeepFilterNet/tree/v0.5.6/models>
- SHA-256: `5998e58e8ba0e09bb76986ef97b84afa065a571ef282d4a1222f341e3251cf3a`
- Upstream repository license: MIT or Apache-2.0

The application must validate the packaged bytes against `manifest.json` before constructing a
processor. Phase 0's offline inference spike performs that check; the reusable asset loader is a
later backlog item.
