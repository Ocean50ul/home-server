**Server startup**

`cargo run server dry-start` will run the server without scaning audio lib, resampling and synchronizing DB state with the music lib state.

**Running Tests.**

To run the tests suit, you need first to prepare fixtures, which includes creating dummy audio files and folders with stripped permissions.

// FIXTURES CREATION NOT WORKING YET, SRY

`cargo run prepare-fixtures` will create all the necessary things inside ./test-fixtures.

`cargo run cleanup-fixtures` will return all the permissions and delete all the fixtures. 

// FIXTURES CREATION NOT WORKING YET, SRY

`cargo test` will run the test suite. If there is no fixtures, it will skip all the tests that was dependent on those fixutres and print out warnings.

For now, the only target for tests is Windows.

**FFMPEG**

This project uses ffmpeg binary (`home-server/ffmpeg/ffmpeg.exe`) to resample audio files which are above 88200hz (thats threshold above which html `<audio>` tag can't do shit about) and to create dummy test fixtures. FFmpeg is licensed under the GNU Lesser General Public License (LGPL).

The tool was downloaded from https://ffmpeg.org/

TODO: shipping .exe is kinda sus, so i will make a script that downloads a thing from a trustworthy mirror and verify it with checksums.
