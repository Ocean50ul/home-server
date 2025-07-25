
// async function test_flac() {
//     let audioCtx = new (window.AudioContext || window.webkitAudioContext)();

//     const response = await fetch("http://192.168.0.71:8080/test_stream");
//     const data = await response.arrayBuffer();

//     const audioBuffer = await audioCtx.decodeAudioData(data);
//     console.log("Decoded successfully", audioBuffer);

//     const source = audioCtx.createBufferSource();
//     source.buffer = audioBuffer;
//     source.connect(audioCtx.destination);
//     source.start();
// }

// async function getTracks() {
//     try {
//         const response = await fetch('http://192.168.0.71:8080/static/tracks.json');
//         if (!response.ok) {
//         throw new Error(`HTTP error! Status: ${response.status}`);
//         }
//         const tracksData = await response.json();

//         // Now tracksData contains your parsed JSON
//         // You can access tracks like: tracksData["1"], tracksData["2"], etc.

//         return tracksData;
//     } catch (error) {
//         console.error('Error fetching tracks:', error);
//         return null;
//     }
// }

// async function setUpTracks() {
//     const tracksData = await getTracks();

//     for (const [id, track] of Object.entries(tracksData)) {
//         const li = document.createElement('li');
//         li.className = 'song-item';
//         li.setAttribute('id', id);
//         li.textContent = `${track.artist} - ${track.title}`;
//         song_list.appendChild(li);
//     }
// }

// function makeFolderGlow(folderId) {
//     const activeFolder = document.getElementById(folderId);

//     if (activeFolder) {
//         activeFolder.classList.add('active');
//     }
// }

// // Function to change the song
// function playSong(song) {
//     // Update the song title in the player
//     document.querySelector('.player-song').textContent = song;

//     // Create a new audio element to play the selected song
//     const audioPlayer = new Audio(song);
//     audioPlayer.play();

// // Optionally, you can add more logic for controlling the player (play, pause, etc.)
// }

// // Function to display the section and add active class
// function showSection(sectionId) {
//     // Hide all sections
//     sections.forEach(section => {
//         section.style.display = "none";
//     });

//     // Show the selected section
//     const sectionToShow = document.getElementById(sectionId);
//     if (sectionToShow) {
//         sectionToShow.style.display = "flex";
//     }

//     // Remove active class from all folders
//     folders.forEach(folder => {
//         folder.classList.remove('active');
//     });

//     // Add active class to the clicked folder
//     const activeFolder = document.getElementById(sectionId.toLowerCase());
//     if (activeFolder) {
//         activeFolder.classList.add('active');
//     }
// }

// document.addEventListener("DOMContentLoaded", async function() {
//     const folders = document.querySelectorAll('.folder');  // All folder elements
//     const sections = document.querySelectorAll('.content-section');  // All section elements

//     const song_list = document.querySelector('.songs-ul');
//     var audioPlayer = document.getElementById('player');

//     document.getElementById("startAudio").addEventListener("click", async () => {
//         await test_flac();
//     })

//     await setUpTracks();


//     // Add click event listeners to each folder
//     folders.forEach(folder => {
//         folder.addEventListener('click', function() {
//             const sectionId = this.id + '-section';  // e.g., "music-section", "videos-section", "files-section"
//             showSection(sectionId);
//             makeFolderGlow(this.id);

//             // Add a paragraph indicating the section that was clicked
//         });
//     });

//     const songs = document.querySelectorAll('.song-item');

//     songs.forEach(song => {
//         song.addEventListener('click', function() {
//             audioPlayer.src = `/static/mp3/${song.id}.mp3`;
//             audioPlayer.load();
//             audioPlayer.play();

//         })
//     })

//     // Initialize the first section to be displayed (Music section as default)
//     showSection('music-section');
//     makeFolderGlow('music');
// });

