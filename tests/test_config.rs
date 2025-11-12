// tests/test_config.rs

use config::{Config as ConfigBuilder, File};
use std::fs;

// Импортируем нашу основную структуру Config из библиотеки
use tracker_updater::Config;

/// Вспомогательная функция для создания временного конфиг-файла
fn create_temp_config(filename: &str, content: &str) {
    fs::write(filename, content).expect("Не удалось создать временный конфиг");
}

/// Вспомогательная функция для удаления временного конфиг-файла
fn cleanup_temp_config(filename: &str) {
    let _ = fs::remove_file(filename); // Игнорируем ошибку, если файла нет
}

#[test]
fn test_load_config_success() {
    let filename = "config.temp_success.toml";
    let content = r#"
        dry_run = true

        [qbit]
        url = "http://test-url.com"
        username = "test_user"
        password = "test_pass"

        [rutracker]
        bb_session_cookie = "test_cookie_123"
    "#;

    create_temp_config(filename, content);

    // Эта логика повторяет src/main.rs
    let builder = ConfigBuilder::builder().add_source(File::with_name(filename).required(true));

    let settings = builder.build().expect("Не удалось собрать конфиг");
    let config = settings
        .try_deserialize::<Config>()
        .expect("Не удалось десериализовать конфиг");

    // Проверяем, что все поля загрузились
    assert!(config.dry_run);
    assert_eq!(config.qbit.url, "http://test-url.com");
    assert_eq!(config.qbit.username, "test_user");
    assert_eq!(config.rutracker.bb_session_cookie, "test_cookie_123");

    cleanup_temp_config(filename);
}

#[test]
fn test_load_config_missing_file() {
    let filename = "config.non_existent.toml";

    // Убедимся, что файла нет
    cleanup_temp_config(filename);

    let builder = ConfigBuilder::builder().add_source(File::with_name(filename).required(true));

    // Ожидаем ошибку при .build()
    let result = builder.build();
    assert!(result.is_err());

    let error_string = result.err().unwrap().to_string();

    assert!(
        error_string.contains(filename) && error_string.contains("not found"),
        "Ожидаемый текст ошибки ('...not found') не совпал. Получено: {}",
        error_string
    );
}

#[test]
fn test_load_config_missing_field() {
    let filename = "config.temp_missing.toml";
    let content = r#"
        dry_run = false

        [qbit]
        url = "http://test-url.com"
        # Поле 'username' отсутствует

        [rutracker]
        bb_session_cookie = "test_cookie_123"
    "#;

    create_temp_config(filename, content);

    let builder = ConfigBuilder::builder().add_source(File::with_name(filename).required(true));

    let settings = builder.build().expect("Не удалось собрать конфиг");

    // Ожидаем ошибку при .try_deserialize()
    let config_result = settings.try_deserialize::<Config>();
    assert!(config_result.is_err());

    let error_msg = config_result.err().unwrap().to_string();
    assert!(error_msg.contains("missing field `username`"));

    cleanup_temp_config(filename);
}

#[test]
fn test_load_config_invalid_type() {
    let filename = "config.temp_bad_type.toml";
    let content = r#"
        dry_run = "это_строка_а_не_bool" # <- Неверный тип

        [qbit]
        url = "http://test-url.com"
        username = "test_user"
        password = "test_pass"

        [rutracker]
        bb_session_cookie = "test_cookie_123"
    "#;

    create_temp_config(filename, content);

    let builder = ConfigBuilder::builder().add_source(File::with_name(filename).required(true));

    let settings = builder.build().expect("Не удалось собрать конфиг");

    // Ожидаем ошибку при .try_deserialize()
    let config_result = settings.try_deserialize::<Config>();
    assert!(config_result.is_err());

    let error_msg = config_result.err().unwrap().to_string();
    assert!(error_msg.contains("invalid type: string"));
    assert!(error_msg.contains("expected a boolean"));

    cleanup_temp_config(filename);
}
