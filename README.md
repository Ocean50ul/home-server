##Running Tests.

To run the suit, you need first to prepare fixtures. That includes downloading audio files from freesound.org and creating dirs and files with stripped premissions.

`cargo run prepare-fixtures` will download # audio tracks and create all the dirs necessary inside ./test-fixtures.
`cargo run cleanup-fixtures` will return all the permissions and clean things up. 

`cargo test` will run the test suite. If there is no fixtures, it will skip all the tests that was dependent on those fixutres and print out warnings.

For now, the only target OS is Windows. 
