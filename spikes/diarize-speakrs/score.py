"""Map each ground-truth turn to the predicted speaker with the largest overlap."""
import re, sys
truth = [l.split() for l in open(sys.argv[1]) if l.strip()]
pred = [(float(a), float(b), s) for a, b, s in re.findall(r"^\s+([\d.]+) -\s+([\d.]+)\s+(\S+)$", sys.stdin.read(), re.M)]
seq = []
for a, b, spk in truth:
    a, b = float(a), float(b)
    best = max(pred, key=lambda p: max(0.0, min(b, p[1]) - max(a, p[0])), default=None)
    seq.append(f"{spk}->{best[2][-2:] if best else '??'}")
print(" ".join(seq))
