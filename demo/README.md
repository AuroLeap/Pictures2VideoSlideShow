# Demo — see the slideshow engine work in one click

This folder is a self-contained example for **`make_video_slideshow.exe`**. It shows
you a *valid input layout* — a folder of media, a configuration file, and a Ken Burns
**focus definition** — and turns it into a slideshow MP4, without you supplying any
media of your own.

## Run it

1. Make sure **`make_video_slideshow.exe`** is in **this same folder**. It already is in
   the extracted release `.zip`. (Building from source instead? Run
   `cargo build --release` and copy `target\release\make_video_slideshow.exe` here.)
2. Double-click **`demo.bat`**.

`demo.bat` will:

1. Download ~12 MB of free-use sample media (see the manifest below) into a new
   **`demo-media\`** folder beside it, verifying each file against a pinned SHA-256
   checksum.
2. Run `make_video_slideshow.exe --config demo_config.toml validate` (this is also where
   a missing FFmpeg is auto-fetched on a clean machine).
3. Run `make_video_slideshow.exe --config demo_config.toml build`.

The finished slideshow lands in **`demo-output\demo-1280x800.mp4`**. Re-running is
instant — already-downloaded media is reused (and re-verified).

## What's in the example

| File | What it shows |
|---|---|
| [`demo_config.toml`](demo_config.toml) | A complete, valid config: media folder, output folder, one 1280×800 / 30 fps / CRF 28 frame definition. Copy and adapt it for your own media. |
| [`focus.json`](focus.json) | The optional **region-of-interest focus database** — it pins the Ken Burns zoom on a chosen spot per image. `cat.jpg` uses a point `{x,y}`; `flower.jpg` uses a bounding box `{bbox:[x,y,w,h]}` (normalized 0–1). Images with no entry fall back to `default_focus` in `demo_config.toml`. |
| `demo-media\` *(created on first run)* | The downloaded sample photos & video clips. |
| `demo-output\` *(created on first run)* | The generated slideshow MP4. |

See the full config-field reference and the focus-database details in the
[Quick Reference](../docs/quick-reference.md) (§4 and §6).

## Sample media manifest (all free-use)

All sample media is downloaded from **Wikimedia Commons** and is public-domain or CC0
(no attribution required); sources are listed here for transparency. The pinned SHA-256
checksums live in `demo.bat`.

| Saved as | Source (Wikimedia Commons) | License |
|---|---|---|
| `cat.jpg` | [Tabby cat with blue eyes-3336579.jpg](https://commons.wikimedia.org/wiki/File:Tabby_cat_with_blue_eyes-3336579.jpg) | CC0 1.0 |
| `flower.jpg` | [Chaenomeles japonica CC0 1.0.jpg](https://commons.wikimedia.org/wiki/File:Chaenomeles_japonica_CC0_1.0.jpg) | CC0 1.0 |
| `sunflower.jpg` | [Red sunflower.jpg](https://commons.wikimedia.org/wiki/File:Red_sunflower.jpg) | Public domain |
| `clouds1.webm` | [Wave clouds on the lee of the Rocky Mountains (CIRA 2019-11-19).webm](https://commons.wikimedia.org/wiki/File:Wave_clouds_on_the_lee_of_the_Rocky_Mountains_(CIRA_2019-11-19).webm) | Public domain (NOAA/CIRA) |
| `clouds2.webm` | [Waves in the Clouds off the Northern California Coast (CIRA 2025-07-15 - nolabels).webm](https://commons.wikimedia.org/wiki/File:Waves_in_the_Clouds_off_the_Northern_California_Coast_(CIRA_2025-07-15_-_nolabels).webm) | Public domain (NOAA/CIRA) |

The two `clouds*.webm` clips are silent; `demo_config.toml` sets `enable_audio = true`
anyway so you can see the option — swap in your own audio-bearing video and its sound
carries into the slideshow.
