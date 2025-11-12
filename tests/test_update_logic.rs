// tests/test_update_logic.rs
// Раздельные запуски
// cargo test --test test_update_logic -- test_full_update_scenario --ignored --nocapture
// cargo test --test test_update_logic -- test_update_preserves_category_and_tags --ignored --nocapture
// cargo test --test test_update_logic -- test_dry_run_scenario --ignored --nocapture

use config::{Config as ConfigBuilder, File}; // Добавлено для загрузки config.toml
use qbit_rs::{
    model::{AddTorrentArg, Credential, NonEmptyStr, TorrentFile, TorrentSource},
    Qbit,
};


use tracker_updater::{run_helper, rutracker_api, Config}; // Config теперь наша структура из lib.rs
use std::collections::{HashMap, HashSet};
use std::error::Error;
use std::path::Path;
use tokio::fs;

// --- Константы для теста ---
const TORRENT_FILE_1: &str = "tests/test-files/old1.torrent";
const TORRENT_FILE_2: &str = "tests/test-files/old2.torrent";
const SAVE_PATH_1: &str = "tests/temp-downloads/test1";
const SAVE_PATH_2: &str = "tests/temp-downloads/test2";

// --- Вспомогательные функции ---

/// Помощник: собирает Config из config.toml
fn setup_config() -> Config {
    let builder = ConfigBuilder::builder()
        // Загружаем из файла `config.toml`. Он должен быть в корне проекта.
        .add_source(File::with_name("config.toml").required(true));

    let config_settings = builder
        .build()
        .expect("Ошибка загрузки config.toml. Убедитесь, что он существует в корне проекта.");

    // Десериализуем в нашу структуру Config из lib.rs
    config_settings
        .try_deserialize::<Config>()
        .expect("Ошибка парсинга config.toml. Проверьте структуру файла.")
}

/// Помощник: инициализирует клиент Qbit из config.toml
async fn setup_client() -> Qbit {
    // Получаем конфиг
    let config = setup_config();

    // Используем данные из вложенной структуры config.qbit
    let credential = Credential::new(config.qbit.username, config.qbit.password);
    Qbit::new(config.qbit.url.as_str(), credential)
}

/// Помощник: Добавляет тестовый торрент и возвращает его (хеш, ID)
async fn add_test_torrent(
    client: &Qbit,
    torrent_file_path: &str,
    save_path: &str,
) -> Result<(String, String), Box<dyn Error>> {
    // 1. Убедимся, что путь сохранения существует
    fs::create_dir_all(save_path).await?;
    let absolute_save_path = fs::canonicalize(save_path).await?;

    // 2. Читаем .torrent файл
    let torrent_content = fs::read(torrent_file_path).await?;
    let torrent_file = TorrentFile {
        data: torrent_content,
        filename: torrent_file_path.to_string(),
    };
    let torrent_source = TorrentSource::TorrentFiles {
        torrents: vec![torrent_file],
    };

    // 3. Собираем аргументы для qBit
    let arg = AddTorrentArg::builder()
        .source(torrent_source)
        .savepath(absolute_save_path.to_string_lossy().to_string())
        .paused(true.to_string()) // Добавляем на паузе
        .build();

    // 4. Добавляем торрент
    client.add_torrent(arg).await?;

    // 5. Находим добавленный торрент, чтобы получить его хеш и ID
    // Даем qBit секунду на обработку
    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

    let torrents = client.get_torrent_list(Default::default()).await?;

    // Ищем его по каноническому пути сохранения
    for t in torrents {
        if let (Some(t_save_path), Some(t_hash)) = (t.save_path, t.hash) {
            if let Ok(t_abs_path) = fs::canonicalize(t_save_path).await {
                if t_abs_path == absolute_save_path {
                    // Нашли! Теперь получаем ID из его свойств
                    let props = client.get_torrent_properties(&t_hash).await?;
                    let comment = props.comment.unwrap_or_default();
                    let torrent_id = rutracker_api::extract_torrent_id_from_comment(&comment);

                    if torrent_id.is_empty() {
                        panic!(
                            "Торрент {} добавлен, но в его комментарии нет ID rutracker!",
                            torrent_file_path
                        );
                    }

                    log::info!(
                        "Добавлен торрент '{}' (ID: {}, Hash: {})",
                        torrent_file_path,
                        torrent_id,
                        t_hash
                    );
                    return Ok((t_hash, torrent_id));
                }
            }
        }
    }

    Err(format!(
        "Не удалось найти только что добавленный торрент '{}' в списке qBittorrent",
        torrent_file_path
    )
    .into())
}

/// Помощник: Добавляет тестовый торрент С КАТЕГОРИЕЙ И ТЕГАМИ
/// Возвращает (хеш, ID)
async fn add_test_torrent_with_metadata(
    client: &Qbit,
    torrent_file_path: &str,
    save_path: &str,
    category: &str,
    tags: &str,
) -> Result<(String, String), Box<dyn Error>> {
    // 1. Убедимся, что путь сохранения существует
    fs::create_dir_all(save_path).await?;
    let absolute_save_path = fs::canonicalize(save_path).await?;

    // 2. Читаем .torrent файл
    let torrent_content = fs::read(torrent_file_path).await?;
    let torrent_file = TorrentFile {
        data: torrent_content,
        filename: torrent_file_path.to_string(),
    };
    let torrent_source = TorrentSource::TorrentFiles {
        torrents: vec![torrent_file],
    };

    // 3. Собираем аргументы для qBit (С КАТЕГОРИЕЙ И ТЕГАМИ)
    let arg = AddTorrentArg::builder()
        .source(torrent_source)
        .savepath(absolute_save_path.to_string_lossy().to_string())
        .paused(true.to_string()) // Добавляем на паузе
        //.category(category.to_string()) // <-- ДОБАВЛЕНО
        //.tags(tags.to_string())         // <-- ДОБАВЛЕНО
        .build();

    // 4. Добавляем торрент
    client.add_torrent(arg).await?;

    // 5. Находим добавленный торрент, чтобы получить его хеш и ID
    // Даем qBit секунду на обработку
    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

    let torrents = client.get_torrent_list(Default::default()).await?;

    // Ищем его по каноническому пути сохранения
    for t in torrents {
        if let (Some(t_save_path), Some(t_hash)) = (t.save_path.as_ref(), t.hash.as_ref()) {
            if let Ok(t_abs_path) = fs::canonicalize(t_save_path).await {
                if t_abs_path == absolute_save_path {
                    // Нашли! Теперь получаем ID
                    let t_hash_cloned = t_hash.clone(); // Клонируем хеш для API вызовов

                    let props = client.get_torrent_properties(t_hash).await?;
                    let comment = props.comment.unwrap_or_default();
                    let torrent_id = rutracker_api::extract_torrent_id_from_comment(&comment);

                    if torrent_id.is_empty() {
                        panic!(
                            "Торрент {} добавлен, но в его комментарии нет ID rutracker!",
                            torrent_file_path
                        );
                    }

                    // --- (НОВЫЙ БЛОК) ---
                    // Устанавливаем метаданные отдельными вызовами
                    log::info!(
                        "    -> Торрент {} найден (хеш: {}). Установка метаданных...",
                        torrent_file_path,
                        t_hash_cloned
                    );

                    let category_non_empty = NonEmptyStr::new(category)
                        .expect("Тестовая категория (test-category) не должна быть пустой");

                    match client.add_category(category_non_empty, "").await {
                        Ok(_) => {
                            log::debug!("Категория '{}' успешно создана.", category);
                        }
                        Err(e) => {
                            // (ИЗМЕНЕНО) Конвертируем ошибку в строку
                            let error_string = e.to_string();

                            // Проверяем, содержит ли строка "409" (Conflict)
                            if error_string.contains("409") {
                                // Это "Conflict", означает, что категория УЖЕ СУЩЕСТВУЕТ.
                                // Это нормально, продолжаем.
                                log::debug!(
                                    "Категория '{}' уже существует, пропуск создания (Ошибка: {}).",
                                    category,
                                    error_string
                                );
                            } else {
                                // Любая другая ошибка - это настоящая паника
                                panic!("Не удалось создать/проверить категорию: {:?}", e);
                            }
                        }
                    }

                    client
                        .set_torrent_category(std::slice::from_ref(&t_hash_cloned), category)
                        .await
                        .expect("Не удалось установить категорию");

                    // qbit-rs ожидает `&[String]` для хешей и `&[String]` для тегов
                    client
                        .add_torrent_tags(std::slice::from_ref(&t_hash_cloned), &[tags.to_string()])
                        .await
                        .expect("Не удалось добавить теги");

                    log::info!(
                        "Добавлен торрент '{}' (ID: {}, Hash: {}, Cat: {}, Tags: {})",
                        torrent_file_path,
                        torrent_id,
                        t_hash,
                        category,
                        tags
                    );
                    return Ok((t_hash.clone(), torrent_id));
                }
            }
        }
    }

    Err(format!(
        "Не удалось найти только что добавленный торрент '{}' в списке qBittorrent",
        torrent_file_path
    )
    .into())
}

/// Помощник: Удаляет торренты (с файлами) и директории
async fn cleanup(client: &Qbit, hashes: Vec<String>, save_paths: Vec<&str>) {
    if !hashes.is_empty() {
        log::warn!("Очистка хешей: {:?}", hashes);
        if let Err(e) = client.delete_torrents(hashes, true).await {
            log::error!("Не удалось удалить торренты: {}", e);
        }
    }

    // Даем qBittorrent время отпустить файлы перед удалением директорий
    log::warn!("Пауза 2 сек, ждем освобождения файлов...");
    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

    for path in save_paths {
        if Path::new(path).exists() {
            log::warn!("Очистка директории: {}", path);

            let mut attempts = 0;
            const MAX_ATTEMPTS: u8 = 3;
            while attempts < MAX_ATTEMPTS {
                attempts += 1;
                match fs::remove_dir_all(path).await {
                    Ok(_) => {
                        log::info!(" -> Директория {} успешно удалена", path);
                        break; // Успех
                    }
                    Err(e) => {
                        log::error!(
                            " -> Попытка {}: Не удалось удалить директорию {}: {}",
                            attempts, path, e
                        );
                        if attempts >= MAX_ATTEMPTS {
                            log::error!(" -> ДОСТИГНУТ ЛИМИТ ПОПЫТОК. Пропускаем...");
                            break;
                        }
                        // Ждем 1 секунду перед повторной попыткой
                        tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
                    }
                }
            }
        }
    }
}

// --- ОСНОВНОЙ ИНТЕГРАЦИОННЫЙ ТЕСТ ---

#[tokio::test]
#[ignore] // Этот тест тяжелый и требует config.toml, .torrent файлы и живой qBit
// запуск только этого теста: cargo test --test test_update_logic -- test_full_update_scenario --ignored --nocapture
async fn test_full_update_scenario() {
    // --- 0. ПРОВЕРКА ---
    // Убедимся, что .torrent файлы на месте
    if !Path::new(TORRENT_FILE_1).exists() || !Path::new(TORRENT_FILE_2).exists() {
        panic!(
            "Тестовые файлы ('{}', '{}') не найдены. \
             Пожалуйста, создайте папку 'tests/test-files/' и \
             поместите в нее 'old1.torrent' и 'old2.torrent'",
            TORRENT_FILE_1, TORRENT_FILE_2
        );
    }

    // Инициализируем логгер (он будет писать в `logs/my_app.log`)
    tracker_updater::init_logger();

    // --- 1. НАСТРОЙКА (ИЗМЕНЕНО) ---
    // Больше не нужно .env, функции `setup_*` сами читают config.toml
    log::info!("--- 1. Загрузка конфигурации из config.toml ---");
    let client = setup_client().await;
    let config = setup_config(); // Эта функция теперь читает config.toml

    // Проверяем, что в тесте НЕ включен dry_run
    if config.dry_run {
        panic!("Для запуска 'test_full_update_scenario' необходимо установить 'dry_run = false' в config.toml");
    }

    // --- 1. НАСТРОЙКА ---
    // Сначала очистим qBit от любых предыдущих неудачных запусков
    log::info!("--- 1. Начальная очистка ---");
    let all_torrents = client.get_torrent_list(Default::default()).await.unwrap();
    let mut hashes_to_delete = Vec::new();
    let path1_abs = fs::canonicalize(SAVE_PATH_1).await.unwrap_or_default();
    let path2_abs = fs::canonicalize(SAVE_PATH_2).await.unwrap_or_default();

    for t in all_torrents {
        if let Some(path_str) = t.save_path {
            if let Ok(abs_path) = fs::canonicalize(path_str).await {
                if abs_path == path1_abs || abs_path == path2_abs {
                    if let Some(hash) = t.hash {
                        hashes_to_delete.push(hash);
                    }
                }
            }
        }
    }
    cleanup(&client, hashes_to_delete, vec![SAVE_PATH_1, SAVE_PATH_2]).await;

    log::info!("--- 1. Фаза Настройки (Добавление торрентов) ---");
    // Добавляем старые торренты
    let (old_hash_1, id_1) = add_test_torrent(&client, TORRENT_FILE_1, SAVE_PATH_1)
        .await
        .expect("Не удалось добавить old1.torrent");

    let (old_hash_2, id_2) = add_test_torrent(&client, TORRENT_FILE_2, SAVE_PATH_2)
        .await
        .expect("Не удалось добавить old2.torrent");

    let old_hashes: HashSet<String> = [old_hash_1.clone(), old_hash_2.clone()]
        .iter()
        .cloned()
        .collect();
    let old_ids: HashSet<String> = [id_1.clone(), id_2.clone()].iter().cloned().collect();

    // --- 2. ВЫПОЛНЕНИЕ ---
    log::info!("--- 2. Фаза Выполнения (Запуск run_helper) ---");

    // Запускаем основную логику
    // Передаем `config`, загруженный из config.toml
    let result = run_helper(config).await;

    // Проверяем, что сам хелпер отработал без паники
    assert!(
        result.is_ok(),
        "run_helper завершился с ошибкой: {:?}",
        result.err()
    );

    log::info!("--- Пауза 2 сек, даем qBit прочитать метаданные... ---");
    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

    log::info!("--- 3. Фаза Проверки ---");

    // --- 3. ПРОВЕРКА ---
    // Получаем финальный список торрентов из qBittorrent
    let final_torrents = client.get_torrent_list(Default::default()).await.unwrap();

    // Собираем карты [Hash -> (ID, Path)] и [ID -> (Hash, Path)]
    let mut final_hashes_map: HashMap<String, (String, std::path::PathBuf)> = HashMap::new();
    let mut final_ids_map: HashMap<String, (String, std::path::PathBuf)> = HashMap::new();
    let mut hashes_for_cleanup = Vec::new();

    let abs_save_path_1 = fs::canonicalize(SAVE_PATH_1).await.unwrap();
    let abs_save_path_2 = fs::canonicalize(SAVE_PATH_2).await.unwrap();

    for t in &final_torrents {
        if let (Some(hash), Some(save_path)) = (t.hash.as_ref(), t.save_path.as_ref()) {
            if let Ok(abs_path) = fs::canonicalize(save_path).await {
                // Ищем только торренты в наших тестовых папках
                if abs_path == abs_save_path_1 || abs_path == abs_save_path_2 {
                    hashes_for_cleanup.push(hash.clone());
                    let props = client.get_torrent_properties(hash).await.unwrap();
                    let id = rutracker_api::extract_torrent_id_from_comment(
                        &props.comment.unwrap_or_default(),
                    );

                    final_hashes_map.insert(hash.clone(), (id.clone(), abs_path.clone()));
                    final_ids_map.insert(id, (hash.clone(), abs_path));
                }
            }
        }
    }

    // 3a. Проверяем, что СТАРЫЕ хеши ИСЧЕЗЛИ
    for old_hash in &old_hashes {
        assert!(
            !final_hashes_map.contains_key(old_hash),
            "Старый хеш {} НЕ был удален!",
            old_hash
        );
    }

    // 3b. Проверяем, что торренты с НУЖНЫМИ ID существуют, у них НОВЫЕ хеши и СТАРЫЕ пути
    for old_id in &old_ids {
        let (new_hash, new_save_path) = final_ids_map
            .get(old_id)
            .unwrap_or_else(|| panic!("Торрент с ID {} отсутствует после обновления!", old_id));

        // Проверяем, что хеш изменился
        assert!(
            !old_hashes.contains(new_hash),
            "Торрент ID {} не обновился (хеш {} совпадает со старым)",
            old_id,
            new_hash
        );

        // Проверяем, что путь сохранения сохранился
        let expected_path = if *old_id == id_1 {
            &abs_save_path_1
        } else {
            &abs_save_path_2
        };
        assert_eq!(
            new_save_path, expected_path,
            "Торрент ID {} обновился, но путь сохранения изменился! (Ожидали: {:?}, Получили: {:?})",
            old_id, expected_path, new_save_path
        );

        log::info!(
            "✅ Проверка пройдена для ID {} (Новый Хеш: {})",
            old_id,
            new_hash
        );
    }

    // --- 4. ОЧИСТКА ---
    log::info!("--- 4. Фаза Очистки ---");
    cleanup(&client, hashes_for_cleanup, vec![SAVE_PATH_1, SAVE_PATH_2]).await;

    log::info!("--- Тест успешно завершен ---");
}

#[tokio::test]
#[ignore] // Этот тест тяжелый и требует config.toml, .torrent файлы и живой qBit
// cargo test --test test_update_logic -- test_update_preserves_category_and_tags --ignored --nocapture
async fn test_update_preserves_category_and_tags() {
    // --- 0. КОНСТАНТЫ ---
    const TEST_CATEGORY: &str = "test-category";
    // qBit хранит теги как одну строку через запятую
    const TEST_TAGS: &str = "tag1, tag2, test";

    // --- 0. ПРОВЕРКА ФАЙЛОВ ---
    if !Path::new(TORRENT_FILE_1).exists() || !Path::new(TORRENT_FILE_2).exists() {
        panic!(
            "Тестовые файлы ('{}', '{}') не найдены.",
            TORRENT_FILE_1, TORRENT_FILE_2
        );
    }

    // Инициализируем логгер
    tracker_updater::init_logger();

    // --- 1. НАСТРОЙКА ---
    log::info!("--- 1. Загрузка конфигурации (Preserve Tags Test) ---");
    let client = setup_client().await;
    let config = setup_config();

    // Проверяем, что в тесте НЕ включен dry_run
    if config.dry_run {
        panic!("Для запуска 'test_update_preserves_category_and_tags' необходимо установить 'dry_run = false' в config.toml");
    }

    // --- 1. НАЧАЛЬНАЯ ОЧИСТКА ---
    log::info!("--- 1. Начальная очистка (Preserve Tags Test) ---");
    let all_torrents = client.get_torrent_list(Default::default()).await.unwrap();
    let mut hashes_to_delete = Vec::new();
    let path1_abs = fs::canonicalize(SAVE_PATH_1).await.unwrap_or_default();
    let path2_abs = fs::canonicalize(SAVE_PATH_2).await.unwrap_or_default();

    for t in all_torrents {
        if let Some(path_str) = t.save_path {
            if let Ok(abs_path) = fs::canonicalize(path_str).await {
                if abs_path == path1_abs || abs_path == path2_abs {
                    if let Some(hash) = t.hash {
                        hashes_to_delete.push(hash);
                    }
                }
            }
        }
    }
    cleanup(&client, hashes_to_delete, vec![SAVE_PATH_1, SAVE_PATH_2]).await;

    log::info!("--- 1. Фаза Настройки (Добавление с метаданными) ---");

    // (ИЗМЕНЕНО) Используем новый хелпер для добавления с метаданными
    let (old_hash_1, id_1) = add_test_torrent_with_metadata(
        &client,
        TORRENT_FILE_1,
        SAVE_PATH_1,
        TEST_CATEGORY,
        TEST_TAGS,
    )
    .await
    .expect("Не удалось добавить old1.torrent с метаданными");

    let (old_hash_2, id_2) = add_test_torrent_with_metadata(
        &client,
        TORRENT_FILE_2,
        SAVE_PATH_2,
        TEST_CATEGORY,
        TEST_TAGS,
    )
    .await
    .expect("Не удалось добавить old2.torrent с метаданными");

    let old_hashes: HashSet<String> = [old_hash_1.clone(), old_hash_2.clone()]
        .iter()
        .cloned()
        .collect();
    let old_ids: HashSet<String> = [id_1.clone(), id_2.clone()].iter().cloned().collect();

    // --- 2. ВЫПОЛНЕНИЕ ---
    log::info!("--- 2. Фаза Выполнения (Запуск run_helper) ---");

    let result = run_helper(config).await;
    assert!(
        result.is_ok(),
        "run_helper завершился с ошибкой: {:?}",
        result.err()
    );

    log::info!("--- Пауза 2 сек, даем qBit прочитать метаданные... ---");
    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

    log::info!("--- 3. Фаза Проверки (Сохранение метаданных) ---");

    // --- 3. ПРОВЕРКА ---
    let final_torrents = client.get_torrent_list(Default::default()).await.unwrap();

    // Собираем карты [Hash -> TorrentInfo] и [ID -> Hash]
    let mut final_torrents_map: HashMap<String, qbit_rs::model::Torrent> = HashMap::new();
    let mut final_ids_map: HashMap<String, String> = HashMap::new(); // [ID -> Hash]
    let mut hashes_for_cleanup = Vec::new();

    let abs_save_path_1 = fs::canonicalize(SAVE_PATH_1).await.unwrap_or_default();
    let abs_save_path_2 = fs::canonicalize(SAVE_PATH_2).await.unwrap_or_default();

    for t in final_torrents {
        if let (Some(hash), Some(save_path)) = (t.hash.as_ref(), t.save_path.as_ref()) {
            if let Ok(abs_path) = fs::canonicalize(save_path).await {
                // Ищем только торренты в наших тестовых папках
                if abs_path == abs_save_path_1 || abs_path == abs_save_path_2 {
                    hashes_for_cleanup.push(hash.clone());
                    let props = client.get_torrent_properties(hash).await.unwrap();
                    let id = rutracker_api::extract_torrent_id_from_comment(
                        &props.comment.unwrap_or_default(),
                    );

                    final_torrents_map.insert(hash.clone(), t.clone());
                    if !id.is_empty() {
                        final_ids_map.insert(id, hash.clone());
                    }
                }
            }
        }
    }

    // 3a. Проверяем, что СТАРЫЕ хеши ИСЧЕЗЛИ
    for old_hash in &old_hashes {
        assert!(
            !final_torrents_map.contains_key(old_hash),
            "Старый хеш {} НЕ был удален!",
            old_hash
        );
    }

    for old_id in &old_ids {
        // ПРОВЕРЯЕМ СЦЕНАРИЙ: Торрент был ОБНОВЛЕН
        if let Some(new_hash) = final_ids_map.get(old_id) {
            log::info!("  ➡️ ПРОВЕРКА (Сценарий 'Обновлен'): ID {}", old_id);

            let new_torrent_info = final_torrents_map
                .get(new_hash)
                .unwrap_or_else(|| panic!("Не найден info для нового хеша {}", new_hash));

            // Проверяем, что хеш изменился
            assert!(
                !old_hashes.contains(new_hash),
                "Торрент ID {} не обновился (хеш {} совпадает со старым)",
                old_id,
                new_hash
            );

            // --- ГЛАВНАЯ ПРОВЕРКА ---

            // Проверяем категорию
            assert_eq!(
                new_torrent_info.category.as_deref(),
                Some(TEST_CATEGORY),
                "Категория НЕ сохранилась для ID {} (Ожидали: {}, Получили: {:?})",
                old_id,
                TEST_CATEGORY,
                new_torrent_info.category
            );

            // Проверяем теги
            assert_eq!(
                new_torrent_info.tags.as_deref(),
                Some(TEST_TAGS),
                "Теги НЕ сохранились для ID {} (Ожидали: {}, Получили: {:?})",
                old_id,
                TEST_TAGS,
                new_torrent_info.tags
            );

            log::info!(
                "✅ Проверка метаданных (Cat/Tags) пройдена для ID {} (Новый Хеш: {})",
                old_id,
                new_hash
            );
        }
        // ПРОВЕРЯЕМ СЦЕНАРИЙ: Торрент был УДАЛЕН
        else {
            log::warn!(
                "  ⚠️ ПРОВЕРКА (Сценарий 'Удален'): Торрент с ID {} был удален (не найден в final_ids_map).",
                old_id
            );

            // Проверяем, что директория ДЕЙСТВИТЕЛЬНО удалена
            let expected_path_str = if *old_id == id_1 {
                SAVE_PATH_1
            } else {
                SAVE_PATH_2
            };
            let expected_path = std::path::Path::new(expected_path_str);

            assert!(
                !expected_path.exists(),
                "Торрент ID {} был удален из qBit, но его директория {} НЕ была удалена!",
                old_id,
                expected_path_str
            );
            log::info!("✅ Проверка удаления директории пройдена для ID {}", old_id);
        }
    }

    // --- 4. ОЧИСТКА ---
    log::info!("--- 4. Фаза Очистки (Preserve Tags Test) ---");
    cleanup(&client, hashes_for_cleanup, vec![SAVE_PATH_1, SAVE_PATH_2]).await;

    log::info!("--- Тест сохранения метаданных успешно завершен ---");
}

#[tokio::test]
#[ignore] // Это также интеграционный тест, требующий живого qBit
// запуск командой: cargo test --test test_update_logic -- --ignored --nocapture
// запуск конкретно этого теста: cargo test --test test_update_logic -- test_dry_run_scenario --ignored --nocapture
async fn test_dry_run_scenario() {
    // --- 0. ПРОВЕРКА ФАЙЛОВ ---
    if !Path::new(TORRENT_FILE_1).exists() {
        panic!(
            "Тестовый файл '{}' не найден.",
            TORRENT_FILE_1
        );
    }

    // Инициализируем логгер
    tracker_updater::init_logger();

    // --- 1. НАСТРОЙКА (Dry Run Test) ---
    log::info!("--- 1. Настройка (Dry Run Test) ---");
    let client = setup_client().await;
    
    // Загружаем конфиг...
    let mut config = setup_config();
    // ... И ПРИНУДИТЕЛЬНО ВЫСТАВЛЯЕМ DRY_RUN = TRUE
    config.dry_run = true;
    
    log::warn!("--- 🟢 ПРИНУДИТЕЛЬНАЯ УСТАНОВКА: dry_run = true для этого теста ---");

    // --- 1. Начальная очистка ---
    log::info!("--- 1. Начальная очистка (Dry Run Test) ---");
    let all_torrents = client.get_torrent_list(Default::default()).await.unwrap();
    let mut hashes_to_delete = Vec::new();
    let path1_abs = fs::canonicalize(SAVE_PATH_1).await.unwrap_or_default();

    for t in all_torrents {
        if let Some(path_str) = t.save_path {
            if let Ok(abs_path) = fs::canonicalize(path_str).await {
                if abs_path == path1_abs {
                    if let Some(hash) = t.hash {
                        hashes_to_delete.push(hash);
                    }
                }
            }
        }
    }
    cleanup(&client, hashes_to_delete, vec![SAVE_PATH_1]).await;

    log::info!("--- 1. Добавление тестового торрента (Dry Run Test) ---");
    
    // Добавляем один старый торрент
    let (old_hash_1, _id_1) = add_test_torrent(&client, TORRENT_FILE_1, SAVE_PATH_1)
        .await
        .expect("Не удалось добавить old1.torrent");

    let _old_hashes: HashSet<String> = [old_hash_1.clone()].iter().cloned().collect();

    // --- 2. ВЫПОЛНЕНИЕ ---
    log::info!("--- 2. Фаза Выполнения (Запуск run_helper в режиме Dry Run) ---");

    // Запускаем основную логику с нашим измененным конфигом
    let result = run_helper(config).await;

    // Проверяем, что сам хелпер отработал без паники
    assert!(
        result.is_ok(),
        "run_helper (dry_run) завершился с ошибкой: {:?}",
        result.err()
    );

    log::info!("--- 3. Фаза Проверки (Dry Run Test) ---");

    // --- 3. ПРОВЕРКА ---
    // Получаем финальный список торрентов из qBittorrent
    let final_torrents = client.get_torrent_list(Default::default()).await.unwrap();

    let mut final_hashes_map: HashMap<String, String> = HashMap::new();
    let mut hashes_for_cleanup = Vec::new();

    for t in &final_torrents {
        if let (Some(hash), Some(save_path)) = (t.hash.as_ref(), t.save_path.as_ref()) {
            if let Ok(abs_path) = fs::canonicalize(save_path).await {
                if abs_path == path1_abs {
                    hashes_for_cleanup.push(hash.clone());
                    let props = client.get_torrent_properties(hash).await.unwrap();
                    let id = rutracker_api::extract_torrent_id_from_comment(
                        &props.comment.unwrap_or_default(),
                    );
                    final_hashes_map.insert(hash.clone(), id);
                }
            }
        }
    }

    // --- ГЛАВНАЯ ПРОВЕРКА ---

    // 3a. Проверяем, что в qBit все еще ОДИН торрент
    assert_eq!(
        final_hashes_map.len(),
        1,
        "Ожидался 1 торрент (dry_run), но найдено {}!",
        final_hashes_map.len()
    );

    // 3b. Проверяем, что этот торрент - СТАРЫЙ
    assert!(
        final_hashes_map.contains_key(&old_hash_1),
        "Старый хеш {} должен был остаться в qBit, но он отсутствует!",
        old_hash_1
    );

    log::info!("✅ Проверка Dry Run УСПЕШНА: Старый торрент {} остался на месте.", old_hash_1);
    log::info!("Обновление было корректно пропущено.");

    // --- 4. ОЧИСТКА ---
    log::info!("--- 4. Фаза Очистки (Dry Run Test) ---");
    cleanup(&client, hashes_for_cleanup, vec![SAVE_PATH_1]).await;

    log::info!("--- Тест Dry Run успешно завершен ---");
}
