/project-root
├── Cargo.toml
├── src/
│   ├── main.rs
│   ├── lib.rs
│   ├── api/                # Эндпоинты (handler’ы)
│   │   ├── mod.rs
│   │   └── track.rs, etc.
│   ├── domain/             # Типы и бизнес-логика
│   │   ├── mod.rs
│   │   └── track.rs, album.rs, artist.rs
│   ├── repository/         # Работа с БД
│   │   ├── mod.rs
│   │   └── impls.rs
│   ├── scanner/            # Логика сканирования файловой системы
│   │   ├── mod.rs
│   │   └── scanner.rs
│   ├── sync/               # Сервис синхронизации БД с файловой системой
│   │   └── mod.rs
│   ├── resampler/          # Работа с ffmpeg
│   │   └── mod.rs