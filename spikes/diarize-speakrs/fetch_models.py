"""Download the speakrs CoreML model files at a pinned revision and verify SHA-256.

Data only: nothing downloaded here is executed. File list mirrors
speakrs-0.6.0/src/models.rs (PLDA_FILES, COREML_COMMON_MODEL_STEMS,
COREML_CHUNK_MODEL_STEMS, mlmodelc_files) for ExecutionMode::CoreMl.
"""
import hashlib, json, os, subprocess, sys

REPO = "avencera/speakrs-models"
REV = "67f963b6a1d94fcd4678948237b3cc8bd69bf1d8"
OUT = sys.argv[1]

STEMS = [
    "segmentation-3.0.mlmodelc", "segmentation-3.0-b32.mlmodelc", "segmentation-3.0-b64.mlmodelc",
    "wespeaker-fbank.mlmodelc", "wespeaker-fbank-b32.mlmodelc", "wespeaker-fbank-30s.mlmodelc",
    "wespeaker-multimask-tail-b32.mlmodelc", "wespeaker-voxceleb-resnet34-tail.mlmodelc",
    "wespeaker-voxceleb-resnet34-tail-b3.mlmodelc", "wespeaker-voxceleb-resnet34-tail-b32.mlmodelc",
    "wespeaker-chunk-emb-p1s-w21.mlmodelc", "wespeaker-chunk-emb-p1s-w36.mlmodelc",
    "wespeaker-chunk-emb-p1s-w51.mlmodelc", "wespeaker-chunk-emb-p1s-w81.mlmodelc",
    "wespeaker-chunk-emb-p1s-w111.mlmodelc",
]
MLMODELC_PARTS = ["model.mil", "coremldata.bin", "weights/weight.bin", "analytics/coremldata.bin"]

tree = json.load(open(os.path.join(OUT, ".tree.json")))
index = {x["path"]: x for x in tree if x["type"] == "file"}
wanted = [p for p in index if p.startswith("plda_") or p.endswith(".min_num_samples.txt")]
wanted += [f"{s}/{part}" for s in STEMS for part in MLMODELC_PARTS]

manifest, total = [], 0
for path in sorted(wanted):
    meta = index.get(path)
    if meta is None:
        sys.exit(f"missing in repo tree: {path}")
    dest = os.path.join(OUT, path)
    os.makedirs(os.path.dirname(dest), exist_ok=True)
    expected = (meta.get("lfs") or {}).get("oid")
    if not os.path.exists(dest):
        url = f"https://huggingface.co/{REPO}/resolve/{REV}/{path}"
        subprocess.run(["curl", "-sfL", "--retry", "3", "-o", dest, url], check=True)
    digest = hashlib.sha256(open(dest, "rb").read()).hexdigest()
    if expected and digest != expected:
        sys.exit(f"SHA-256 mismatch: {path}")
    size = os.path.getsize(dest)
    total += size
    manifest.append({"path": path, "size": size, "sha256": digest, "lfs_verified": bool(expected)})

json.dump({"repo": REPO, "revision": REV, "files": manifest}, open(os.path.join(OUT, "manifest.json"), "w"), indent=1)
print(f"{len(manifest)} files, {total/1e6:.1f} MB, lfs-verified: {sum(m['lfs_verified'] for m in manifest)}")
