// build.rs
use std::env;
use std::fs;
use std::path::PathBuf;

fn main() {
    // Мы хотим делать это только при сборке релиза
    if env::var("PROFILE").unwrap() == "release" {
        // 1. Получаем путь к корню проекта (где лежит Cargo.toml)
        let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());

        // 2. Формируем путь к папке target/release
        // (Это немного "грязный" хак, так как путь может быть
        // переопределен, но для 99% случаев сработает)
        let mut target_dir = manifest_dir.clone();
        target_dir.push("target");
        target_dir.push("release");

        // 3. Список файлов для копирования
        let files_to_copy = ["LICENSE-MIT", "README.md", "favicon.ico"];

        for file_name in files_to_copy.iter() {
            let mut source_path = manifest_dir.clone();
            source_path.push(file_name);

            let mut dest_path = target_dir.clone();
            dest_path.push(file_name);

            // Копируем, только если исходный файл существует
            if source_path.exists() {
                fs::copy(&source_path, &dest_path)
                    .unwrap_or_else(|_| panic!("Не удалось скопировать файл: {}", file_name));
            }
        }
    }
}
