-- 0001_create_music_tables.sql
-- Up migration
CREATE TABLE IF NOT EXISTS artists (
    id BLOB PRIMARY KEY NOT NULL,
    name TEXT NOT NULL UNIQUE
);

CREATE TABLE IF NOT EXISTS albums (
    id BLOB PRIMARY KEY NOT NULL,
    name TEXT NOT NULL,
    artist_id BLOB NOT NULL,
    year INTEGER,
    
    UNIQUE(name, artist_id),
    FOREIGN KEY (artist_id) REFERENCES artists(id)
);

CREATE TABLE IF NOT EXISTS tracks (
    id BLOB PRIMARY KEY NOT NULL,
    name TEXT NOT NULL,
    album_id BLOB NOT NULL,
    duration INTEGER NOT NULL CHECK (duration >= 0),
    
    file_path TEXT NOT NULL UNIQUE,
    file_size INTEGER NOT NULL CHECK (file_size >= 0),
    file_type TEXT NOT NULL,
    
    uploaded TEXT NOT NULL CHECK (uploaded IN ('masha', 'denis')),

    date_added TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    
    FOREIGN KEY (album_id) REFERENCES albums(id)
);