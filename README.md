## Server Startup.

Right now there is the only one way to try the server: clone the repository, build it and prepare the environment.

```
git clone https://github.com/Ocean50ul/home-server.git
cargo build
cargo run prepare
cargo run serve
```

Cargo and git are obviously required.

Environment preparation (`cargo run prepare`) includes:
1. Creating directories and DB instance.
2. Downloading ffmpeg archive from a mirror (url is stored inside config.toml, gyan.dev is default one, you can use whatever you want)
3. Archive integrity check (url for sha checksum is also inside config.toml, gyan.dev is default one, you can use whatever you want)
4. Extracting ffmpeg.exe and cleaning things up

Dockerfile and pre-build binaries are coming soon.

## Resampling

**!!!WARNING!!!**

Server is using resampler, since html `<audio>` tag cant handle anything above 88200hz. Right now, it will REPLACE audio tracks inside `./data/media/music/` with resampled ones. 

**I repeat**, all your tracks **inside** `./data/media/music/` that have high sample rate are going to be **REPLACED**, so take care.

Different resample policies are cooming soon.

Also video transcoding \ remuxing is coming. 

## Target OS

The only target OS right now is **WINDOWS**. 

## Test

In order to run test, you will need to prepare fixtures. That includes creating some dummy files with metadata, as well as dirs with stripped permissions.

`cargo run prepare --dev` will create everything necessary for tests to run.

`cargo run prepare --clenup` will clean the fixtures and dirs. This might fail tho, and if so you will end up with some dirs with stripped permissions. All the stripped dir paths should be inside `./test_fixtures/fixtures_state.json`. Sorry for inconviniece.
