#!/usr/bin/env bash
# The SMEAR A/B (reverb TYPEs spec § A/B): QUAD at its defaults, MIX 1, the
# dry send plus the return, on the kick, snare, strings, pad and an IDM
# phrase, into target/ab/<tag>/.
#   render.sh after    here: SMEAR 0, 0.3 and 1; makes the stimuli first.
#   render.sh before   in a worktree at the T4 reference (a2abe82f), with
#                      AB_DIR pointing at this one's target/ab: GRIT 0.
# Once both exist it writes the listening set into $AB_OUT (default
# target/ab/smear): <source>_before.wav, <source>_smear<x>_after.wav,
# manifest.json and smear_report.md.
set -euo pipefail
tag=${1:?usage: render.sh before|after}
root=$(git -C "$(dirname "$0")" rev-parse --show-toplevel)
export AB_DIR=${AB_DIR:-$root/target/ab}
out=${AB_OUT:-$AB_DIR/smear}
sources=${AB_SOURCES:-kick snare strings pad idm}
test_() {
  cargo test -q --release --manifest-path "$root/Cargo.toml" -p chimera-core \
    --test reverb_render_test -- --ignored --exact "$@" >/dev/null
}
case $tag in
  before) smears="0" ;;
  after)
    smears="0 0.3 1"
    python3 "$root/tools/ab/stimuli.py" "$AB_DIR/stimuli"
    test_ stimuli
    ;;
  *) echo "render.sh before|after" >&2; exit 2 ;;
esac
mkdir -p "$AB_DIR/$tag"
for s in $sources; do
  for m in $smears; do
    AB_STIMULUS=$s AB_SMEAR=$m test_ render
    mv "$AB_DIR/${s}_s$m.wav" "$AB_DIR/${s}_s$m.txt" "$AB_DIR/$tag/"
  done
done

[ -d "$AB_DIR/before" ] && [ -d "$AB_DIR/after" ] || exit 0
mkdir -p "$out"
setting() {
  case $1 in
    0) echo "SMEAR 0 (QUAD's default)" ;;
    0.3) echo "SMEAR 0.3 (ROOM, HALL and BLOOM's default)" ;;
    *) echo "SMEAR $1" ;;
  esac
}
{
  echo "["
  sep=""
  for s in $sources; do
    cp "$AB_DIR/before/${s}_s0.wav" "$out/${s}_before.wav"
    for m in 0 0.3 1; do
      cp "$AB_DIR/after/${s}_s$m.wav" "$out/${s}_smear${m}_after.wav"
      printf '%s  {"source": "%s", "setting": "%s", "before": "%s", "after": "%s"}' \
        "$sep" "$s" "$(setting "$m")" "${s}_before.wav" "${s}_smear${m}_after.wav"
      sep=$',\n'
    done
  done
  printf '\n]\n'
} > "$out/manifest.json"
{
  echo "# SMEAR before and after"
  echo
  echo "QUAD at its defaults (SIZE 0.5, TIME 0.5, DAMP 0.3, RATE 0.5 = 0.22 Hz), MIX 1,"
  echo "through FxBus; each WAV is the dry send plus the return, 6 dB down."
  echo "Before: the T4 reference (a2abe82f) at GRIT 0. Spread: a 1 kHz probe's"
  echo "per-pass pitch through QUAD at SIZE 0 and TIME 1 at that SMEAR, the"
  echo "largest distance from its mean, in cents (the setting's, the same for"
  echo "every source). Peak and last 2 s in dBFS before the 6 dB."
  echo
  echo "| WAV | setting | SMEAR 0 = before | spread | peak | last 2 s |"
  echo "|---|---|---|---|---|---|"
  for s in $sources; do
    IFS=$'\t' read -r _ _ spread pk last < "$AB_DIR/before/${s}_s0.txt"
    echo "| ${s}_before.wav | GRIT 0 (T4) | — | $spread | $pk | $last |"
    for m in 0 0.3 1; do
      IFS=$'\t' read -r _ _ spread pk last < "$AB_DIR/after/${s}_s$m.txt"
      same="—"
      if [ "$m" = 0 ]; then
        cmp -s "$AB_DIR/before/${s}_s0.wav" "$AB_DIR/after/${s}_s0.wav" \
          && same="byte-identical" || same="DIFFERS"
      fi
      echo "| ${s}_smear${m}_after.wav | $(setting "$m") | $same | $spread | $pk | $last |"
    done
  done
} > "$out/smear_report.md"
echo "$out"
