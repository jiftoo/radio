Stream mp3 audio to the world. Wrote this for myself to be able to listen to my music collection from anywhere.

Recursively walks a directory and serves all (mp3, flac, opus, wav) files from the directory tree.
Requires `ffmpeg` and `ffprobe`, probably any version as long as it can read the formats above and has libmp3lame enabled.

Here's the output of help as of now:
```
Usage: radio [OPTIONS] --root <ROOT>

Options:
      --generate-config [<FILE>]
          Overwrite existing or create a new config file. Optionally pass a path to the config file to be created (not directory).
          Doesn't work right yet.

      --use-config [<FILE>]
          Use the config file instead of the command line. Generates a new config if none exists.
          All arguments except '--generate-config' are ignored if this is present.
          Optionally pass a path to the config file to be created/read (not directory).

      --host <HOST>
          The host to bind to.
          
          [default: 127.0.0.1]

      --port <PORT>
          [default: 9005]

      --enable-webui
          not implemented.

      --shuffle
          Choose next song randomly.

      --bitrate <TRANSCODE_BITRATE>
          The bitrate to use for transcoding. Plain value for bps and suffixed with 'k' for kbps.
          
          [default: 128k]

      --enable-mediainfo
          Enable /mediainfo endpoint. It serves metadata for the current song in JSON format.

      --mediainfo-history <SIZE>
          The size of song history to keep track of. Must be greater than 0.
          
          [default: 16]

      --transcode-all[=<TRANSCODE_ALL>]
          Transcode files that can be sent without transcoding. Set to true if you want to reduce bandwidth a little.
          
          [default: false]
          [possible values: true, false]

      --root <ROOT>
          The root directory to recursively search for music.
          Note: --use-config allows to specify multiple root directories.

      --include <INCLUDE>
          Include these directories or files.

      --exclude <EXCLUDE>
          Exclude these directories or files.

  -h, --help
          Print help (see a summary with '-h')

  -V, --version
          Print version

```
---
Here's a lucky commit hash I pulled:
<img src="https://github.com/jiftoo/radio/assets/39745401/11d37085-8092-4d7e-9e9a-cde4de063d0d" />
