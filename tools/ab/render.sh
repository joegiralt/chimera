#!/usr/bin/env bash
# The GRIT A/B (reverb TYPEs spec § A/B): QUAD's defaults on the saw chord
# and the snare at GRIT 0, 0.3, 0.6 and 1, into target/ab/<tag>/. Run it
# as `before` in a worktree at the old GRIT and as `after` here, with
# AB_DIR pointing both at the same target/ab; once both exist it writes
# target/ab/grit_report.md.
set -euo pipefail
tag=${1:?usage: render.sh <tag>}
root=$(git -C "$(dirname "$0")" rev-parse --show-toplevel)
export AB_DIR=${AB_DIR:-$root/target/ab}
[ -d "$AB_DIR/stimuli" ] || python3 "$root/tools/ab/stimuli.py" "$AB_DIR/stimuli"
mkdir -p "$AB_DIR/$tag"
for s in ${AB_STIMULI:-saw_chord snare}; do
  for g in 0 0.3 0.6 1; do
    AB_STIMULUS=$s AB_GRIT=$g cargo test -q --release --manifest-path "$root/Cargo.toml" \
      -p chimera-core --test reverb_render_test -- --ignored --exact render >/dev/null
    mv "$AB_DIR/${s}_g$g.wav" "$AB_DIR/$tag/${s}_g$g.wav"
    mv "$AB_DIR/${s}_g$g.txt" "$AB_DIR/$tag/${s}_g$g.txt"
  done
done

[ -d "$AB_DIR/before" ] && [ -d "$AB_DIR/after" ] || exit 0
{
  echo "# GRIT before and after"
  echo
  echo "QUAD at its defaults, MIX 1, dry off, through FxBus. Floor: noise under a"
  echo "steady −12 dBFS 1 kHz sine, 20 Hz–10.5 kHz, dBFS. Products: 2 and 3 kHz"
  echo "over the median ±200 Hz in a 1 kHz burst's 4 s tail, summed over 100 ms"
  echo "windows (worst window in brackets), dB. Peak and last 2 s: the render's,"
  echo "dBFS; the WAVs are written 6 dB down, as MIX 1 peaks at 0 dBFS."
  echo
  echo "| WAV | GRIT | floor | products | peak | last 2 s |"
  echo "|---|---|---|---|---|---|"
  for s in ${AB_STIMULI:-saw_chord snare}; do
    for g in 0 0.3 0.6 1; do
      for t in before after; do
        IFS=$'\t' read -r _ _ floor prod worst peak last < "$AB_DIR/$t/${s}_g$g.txt"
        echo "| $t/${s}_g$g.wav | $g | $floor | $prod ($worst) | $peak | $last |"
      done
    done
  done
} > "$AB_DIR/grit_report.md"
echo "$AB_DIR/grit_report.md"
