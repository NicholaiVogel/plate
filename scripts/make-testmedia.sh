#!/usr/bin/env bash
# Generates synthetic test media for plate into ./testmedia (gitignored):
# H.264+AAC MP4, ProRes 4444+alpha MOV, half-float EXR (ZIP16 and DWAA),
# 10-bit DPX and 16-bit PNG sequences, and a two-reel unencrypted DCP with
# 5.1 PCM sound (WAV-wrapped; see below).
#
# Usage: scripts/make-testmedia.sh [outdir]
set -euo pipefail

out="${1:-testmedia}"
mkdir -p "$out"/{exr_half,exr_dwaa,dpx10,png16,dcp}
cd "$out"
ff() { ffmpeg -v error -y "$@"; }

ff -f lavfi -i testsrc2=size=1920x1080:rate=24 -f lavfi -i "sine=frequency=440:sample_rate=48000" \
   -t 5 -c:v libx264 -pix_fmt yuv420p -c:a aac clip_h264.mp4
ff -f lavfi -i testsrc2=size=1920x1080:rate=24 -f lavfi -i "sine=frequency=660:sample_rate=48000" \
   -t 3 -vf format=yuva444p10le -c:v prores_ks -profile:v 4444 -c:a pcm_s24le clip_prores4444.mov
ff -f lavfi -i testsrc2=size=2048x1152:rate=24 -t 2 -vf format=gbrpf32le \
   -c:v exr -format half -compression zip16 -start_number 1001 exr_half/shot_v001.%04d.exr
ff -f lavfi -i testsrc2=size=2048x1152:rate=24 -t 2 -vf format=gbrp10le \
   -c:v dpx -start_number 1001 dpx10/plate.%04d.dpx
ff -f lavfi -i testsrc2=size=1280x720:rate=24 -t 1 -vf format=rgb48be -start_number 1 png16/img_%03d.png

if command -v oiiotool >/dev/null; then
  for f in exr_half/*.exr; do
    oiiotool "$f" --compression dwaa -o "exr_dwaa/$(basename "$f")"
  done
fi

# --- DCP: two 2-second reels, JPEG 2000 X'Y'Z' picture + 6ch 24-bit PCM ---
dcp=dcp
uuid() { cat /proc/sys/kernel/random/uuid; }
reels=()
for n in 1 2; do
  pid=$(uuid); sid=$(uuid)
  ff -f lavfi -i "testsrc2=size=2048x858:rate=24" -t 2 \
     -vf "drawtext=text='REEL $n':fontsize=120:fontcolor=white:x=(w-tw)/2:y=(h-th)/2,format=xyz12le" \
     -c:v jpeg2000 -f mxf "$dcp/picture_$n.mxf"
  # FFmpeg can't mux audio-only MXF (real DCP sound essence comes from
  # asdcplib); a WAV stands in, since the ASSETMAP doesn't care.
  ff -f lavfi -i "sine=frequency=$((300 * n)):sample_rate=48000" -t 2 \
     -af "pan=5.1|FL=c0|FR=c0|FC=c0|LFE=0*c0|BL=0*c0|BR=0*c0" -c:a pcm_s24le "$dcp/sound_$n.wav"
  reels+=("$pid $sid $n")
done
cpl=$(uuid); pkl=$(uuid)
{
  echo '<?xml version="1.0" encoding="UTF-8"?>'
  echo '<CompositionPlaylist xmlns="http://www.smpte-ra.org/schemas/429-7/2006/CPL">'
  echo "  <Id>urn:uuid:$cpl</Id><ContentTitleText>PLATE_TEST_FTR</ContentTitleText><ReelList>"
  for r in "${reels[@]}"; do
    set -- $r
    echo "    <Reel><Id>urn:uuid:$(uuid)</Id><AssetList>"
    echo "      <MainPicture><Id>urn:uuid:$1</Id><EditRate>24 1</EditRate><IntrinsicDuration>48</IntrinsicDuration><EntryPoint>0</EntryPoint><Duration>48</Duration><FrameRate>24 1</FrameRate></MainPicture>"
    echo "      <MainSound><Id>urn:uuid:$2</Id><EditRate>24 1</EditRate><IntrinsicDuration>48</IntrinsicDuration><EntryPoint>0</EntryPoint><Duration>48</Duration></MainSound>"
    echo "    </AssetList></Reel>"
  done
  echo '  </ReelList></CompositionPlaylist>'
} > "$dcp/CPL_$cpl.xml"
{
  echo '<?xml version="1.0" encoding="UTF-8"?>'
  echo '<AssetMap xmlns="http://www.smpte-ra.org/schemas/429-9/2007/AM"><Id>urn:uuid:'"$(uuid)"'</Id><AssetList>'
  echo "  <Asset><Id>urn:uuid:$cpl</Id><ChunkList><Chunk><Path>CPL_$cpl.xml</Path></Chunk></ChunkList></Asset>"
  for r in "${reels[@]}"; do
    set -- $r
    echo "  <Asset><Id>urn:uuid:$1</Id><ChunkList><Chunk><Path>picture_$3.mxf</Path></Chunk></ChunkList></Asset>"
    echo "  <Asset><Id>urn:uuid:$2</Id><ChunkList><Chunk><Path>sound_$3.wav</Path></Chunk></ChunkList></Asset>"
  done
  echo '</AssetList></AssetMap>'
} > "$dcp/ASSETMAP.xml"

echo "test media in $(pwd)"

# A real SMPTE DCP (asdcplib-wrapped picture and sound) when DCP-o-matic is installed.
if command -v dcpomatic2_create >/dev/null && command -v dcpomatic2_cli >/dev/null; then
  rm -rf dcpomatic_film
  dcpomatic2_create -n PLATE_DCPOMATIC --no-use-isdcf-name -c TST --twok --container-ratio 178 \
    -a 6 -o dcpomatic_film clip_h264.mp4 >/dev/null
  dcpomatic2_cli dcpomatic_film >/dev/null
  echo "real DCP in $(pwd)/dcpomatic_film/PLATE_DCPOMATIC"
fi
