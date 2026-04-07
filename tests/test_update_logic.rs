// tests/test_update_logic.rs
// Раздельные запуски
// cargo test --test test_update_logic -- test_full_update_scenario --ignored --nocapture
// cargo test --test test_update_logic -- test_update_preserves_category_and_tags --ignored --nocapture
// cargo test --test test_update_logic -- test_dry_run_scenario --ignored --nocapture
// cargo test --test test_update_logic -- --ignored --test-threads=1

use config::{Config as ConfigBuilder, File};
use qbit_rs::{
    model::{AddTorrentArg, Credential, NonEmptyStr, TorrentFile, TorrentSource},
    Qbit,
};
use std::sync::Once;

use std::collections::{HashMap, HashSet};
use std::error::Error;
use std::path::Path;
use tokio::fs;
use tracker_updater::{run_helper, rutracker_api, Config};

// --- Константы для теста ---
const TORRENT_FILE_1: &str = "tests/test-files/old1.torrent";
const TORRENT_FILE_2: &str = "tests/test-files/old2.torrent";
const SAVE_PATH_1: &str = "tests/temp-downloads/test1";
const SAVE_PATH_2: &str = "tests/temp-downloads/test2";

// Изолирующие теги и категории
const TEST_TAG: &str = "test-update";
const TEST_CATEGORY: &str = "test-updater-category";

// --- Вспомогательные функции ---

fn setup_config() -> Config {
    let builder =
        ConfigBuilder::builder().add_source(File::with_name("config.toml").required(true));

    let config_settings = builder
        .build()
        .expect("Ошибка загрузки config.toml. Убедитесь, что он существует в корне проекта.");

    // Делаем config изменяемым (mut), чтобы переопределить настройки для тестов
    let mut config: Config = config_settings
        .try_deserialize::<Config>()
        .expect("Ошибка парсинга config.toml. Проверьте структуру файла.");

    // ПРИНУДИТЕЛЬНО устанавливаем фильтр по тегу для тестов.
    // Это гарантирует, что:
    // 1. Тесты не будут трогать ваши реальные торренты (даже если в config.toml фильтра нет).
    // 2. Тесты всегда найдут тестовые торренты (даже если в config.toml указан другой тег).
    config.tag_filter = Some(TEST_TAG.to_string());

    config
}

async fn setup_client() -> Qbit {
    let config = setup_config();
    let credential = Credential::new(config.qbit.username, config.qbit.password);
    Qbit::new(config.qbit.url.as_str(), credential)
}

static INIT_LOGGER: Once = Once::new();

fn setup_logger() {
    INIT_LOGGER.call_once(|| {
        tracker_updater::init_logger();
    });
}

/// Помощник: Добавляет тестовый торрент С КАТЕГОРИЕЙ И ТЕГАМИ
async fn add_test_torrent(
    client: &Qbit,
    torrent_file_path: &str,
    save_path: &str,
    category: &str,
    tags: &str,
) -> Result<(String, String), Box<dyn Error>> {
    fs::create_dir_all(save_path).await?;
    let absolute_save_path = fs::canonicalize(save_path).await?;

    let torrent_content = fs::read(torrent_file_path).await?;
    let torrent_file = TorrentFile {
        data: torrent_content,
        filename: torrent_file_path.to_string(),
    };
    let torrent_source = TorrentSource::TorrentFiles {
        torrents: vec![torrent_file],
    };

    let arg = AddTorrentArg::builder()
        .source(torrent_source)
        .savepath(absolute_save_path.to_string_lossy().to_string())
        .paused(true.to_string())
        .build();

    client.add_torrent(arg).await?;

    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

    let torrents = client.get_torrent_list(Default::default()).await?;

    for t in torrents {
        if let (Some(t_save_path), Some(t_hash)) = (t.save_path.as_ref(), t.hash.as_ref()) {
            if let Ok(t_abs_path) = fs::canonicalize(t_save_path).await {
                if t_abs_path == absolute_save_path {
                    let t_hash_cloned = t_hash.clone();

                    let props = client.get_torrent_properties(t_hash).await?;
                    let comment = props.comment.unwrap_or_default();
                    let torrent_id = rutracker_api::extract_torrent_id_from_comment(&comment);

                    if torrent_id.is_empty() {
                        panic!(
                            "В комментарии торрента {} нет ID rutracker!",
                            torrent_file_path
                        );
                    }

                    let category_non_empty = NonEmptyStr::new(category)
                        .expect("Тестовая категория не должна быть пустой");

                    match client.add_category(category_non_empty, "").await {
                        Ok(_) => log::debug!("Категория '{}' создана.", category),
                        Err(e) => {
                            if e.to_string().contains("409") {
                                log::debug!("Категория '{}' уже существует.", category);
                            } else {
                                panic!("Не удалось проверить категорию: {:?}", e);
                            }
                        }
                    }

                    client
                        .set_torrent_category(std::slice::from_ref(&t_hash_cloned), category)
                        .await?;
                    client
                        .add_torrent_tags(std::slice::from_ref(&t_hash_cloned), &[tags.to_string()])
                        .await?;

                    log::info!(
                        "Добавлен торрент '{}' (ID: {}, Hash: {}, Tags: {})",
                        torrent_file_path,
                        torrent_id,
                        t_hash,
                        tags
                    );
                    return Ok((t_hash.clone(), torrent_id));
                }
            }
        }
    }

    Err(format!(
        "Не удалось найти добавленный торрент '{}'",
        torrent_file_path
    )
    .into())
}

/// Помощник: Находит все торренты с тегом TEST_TAG и удаляет их
async fn cleanup_test_torrents(client: &Qbit, save_paths: Vec<&str>) {
    let all_torrents = client
        .get_torrent_list(Default::default())
        .await
        .unwrap_or_default();
    let mut hashes_to_delete = Vec::new();

    // Ищем торренты по тегу
    for t in all_torrents {
        if let Some(tags) = &t.tags {
            if tags.contains(TEST_TAG) {
                if let Some(hash) = &t.hash {
                    hashes_to_delete.push(hash.clone());
                }
            }
        }
    }

    if !hashes_to_delete.is_empty() {
        log::warn!(
            "Очистка хешей по тегу '{}': {:?}",
            TEST_TAG,
            hashes_to_delete
        );
        if let Err(e) = client.delete_torrents(hashes_to_delete, true).await {
            log::error!("Не удалось удалить торренты: {}", e);
        }

        log::warn!("Пауза 2 сек, ждем освобождения файлов...");
        tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
    }

    for path in save_paths {
        if Path::new(path).exists() {
            log::warn!("Очистка директории: {}", path);
            let mut attempts = 0;
            const MAX_ATTEMPTS: u8 = 3;
            while attempts < MAX_ATTEMPTS {
                attempts += 1;
                match fs::remove_dir_all(path).await {
                    Ok(_) => break,
                    Err(e) => {
                        log::error!(
                            " -> Попытка {}: Не удалось удалить {}: {}",
                            attempts,
                            path,
                            e
                        );
                        if attempts >= MAX_ATTEMPTS {
                            break;
                        }
                        tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
                    }
                }
            }
        }
    }
}

// --- ОСНОВНОЙ ИНТЕГРАЦИОННЫЙ ТЕСТ ---

#[tokio::test]
#[ignore]
async fn test_full_update_scenario() {
    if !Path::new(TORRENT_FILE_1).exists() || !Path::new(TORRENT_FILE_2).exists() {
        panic!("Тестовые файлы не найдены.");
    }
    setup_logger();

    let client = setup_client().await;
    let config = setup_config();

    if config.dry_run {
        panic!("Необходимо 'dry_run = false' в config.toml");
    }

    log::info!("--- 1. Начальная очистка ---");
    cleanup_test_torrents(&client, vec![SAVE_PATH_1, SAVE_PATH_2]).await;

    log::info!("--- 1. Фаза Настройки ---");
    let (old_hash_1, id_1) = add_test_torrent(
        &client,
        TORRENT_FILE_1,
        SAVE_PATH_1,
        TEST_CATEGORY,
        TEST_TAG,
    )
    .await
    .unwrap();
    let (old_hash_2, id_2) = add_test_torrent(
        &client,
        TORRENT_FILE_2,
        SAVE_PATH_2,
        TEST_CATEGORY,
        TEST_TAG,
    )
    .await
    .unwrap();

    let old_hashes: HashSet<String> = [old_hash_1.clone(), old_hash_2.clone()]
        .iter()
        .cloned()
        .collect();
    let old_ids: HashSet<String> = [id_1.clone(), id_2.clone()].iter().cloned().collect();

    log::info!("--- 2. Фаза Выполнения ---");
    let result = run_helper(config).await;
    assert!(
        result.is_ok(),
        "run_helper завершился с ошибкой: {:?}",
        result.err()
    );

    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

    log::info!("--- 3. Фаза Проверки ---");
    let final_torrents = client.get_torrent_list(Default::default()).await.unwrap();

    let mut final_hashes_map = HashMap::new();
    let mut final_ids_map = HashMap::new();

    let abs_save_path_1 = fs::canonicalize(SAVE_PATH_1).await.unwrap();
    let abs_save_path_2 = fs::canonicalize(SAVE_PATH_2).await.unwrap();

    for t in &final_torrents {
        // Фильтруем результаты строго по нашему тестовому тегу
        if let Some(tags) = t.tags.as_deref() {
            if tags.contains(TEST_TAG) {
                let hash = t.hash.as_ref().unwrap();
                let save_path = t.save_path.as_ref().unwrap();
                let abs_path = fs::canonicalize(save_path).await.unwrap();

                let props = client.get_torrent_properties(hash).await.unwrap();
                let id = rutracker_api::extract_torrent_id_from_comment(
                    &props.comment.unwrap_or_default(),
                );

                final_hashes_map.insert(hash.clone(), (id.clone(), abs_path.clone()));
                final_ids_map.insert(id, (hash.clone(), abs_path));
            }
        }
    }

    for old_hash in &old_hashes {
        assert!(
            !final_hashes_map.contains_key(old_hash),
            "Старый хеш {} НЕ был удален!",
            old_hash
        );
    }

    for old_id in &old_ids {
        // Убираем жесткий unwrap() - если тема удалена с рутрекера, торрента не будет
        if let Some((new_hash, new_save_path)) = final_ids_map.get(old_id) {
            assert!(
                !old_hashes.contains(new_hash),
                "Торрент ID {} не обновился (хеш остался прежним)",
                old_id
            );

            let expected_path = if *old_id == id_1 {
                &abs_save_path_1
            } else {
                &abs_save_path_2
            };
            assert_eq!(new_save_path, expected_path, "Путь сохранения изменился!");
        } else {
            log::warn!(
                "Торрент ID {} был удален с Rutracker, проверка обновления пропускается",
                old_id
            );
        }
    }

    log::info!("--- 4. Фаза Очистки ---");
    cleanup_test_torrents(&client, vec![SAVE_PATH_1, SAVE_PATH_2]).await;
}

#[tokio::test]
#[ignore]
async fn test_update_preserves_category_and_tags() {
    // Включаем тестовый тег в строку тегов для этого сценария
    let multi_tags = format!("tag1, tag2, {}", TEST_TAG);

    if !Path::new(TORRENT_FILE_1).exists() || !Path::new(TORRENT_FILE_2).exists() {
        panic!("Тестовые файлы не найдены.");
    }
    setup_logger();

    let client = setup_client().await;
    let config = setup_config();

    if config.dry_run {
        panic!("Необходимо 'dry_run = false' в config.toml");
    }

    log::info!("--- 1. Начальная очистка ---");
    cleanup_test_torrents(&client, vec![SAVE_PATH_1, SAVE_PATH_2]).await;

    log::info!("--- 1. Фаза Настройки ---");
    let (old_hash_1, id_1) = add_test_torrent(
        &client,
        TORRENT_FILE_1,
        SAVE_PATH_1,
        TEST_CATEGORY,
        &multi_tags,
    )
    .await
    .unwrap();
    let (old_hash_2, id_2) = add_test_torrent(
        &client,
        TORRENT_FILE_2,
        SAVE_PATH_2,
        TEST_CATEGORY,
        &multi_tags,
    )
    .await
    .unwrap();

    let old_hashes: HashSet<String> = [old_hash_1.clone(), old_hash_2.clone()]
        .iter()
        .cloned()
        .collect();
    let old_ids: HashSet<String> = [id_1.clone(), id_2.clone()].iter().cloned().collect();

    log::info!("--- 2. Фаза Выполнения ---");
    let result = run_helper(config).await;
    assert!(result.is_ok());

    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

    log::info!("--- 3. Фаза Проверки ---");
    let final_torrents = client.get_torrent_list(Default::default()).await.unwrap();

    let mut final_torrents_map = HashMap::new();
    let mut final_ids_map = HashMap::new();

    for t in final_torrents {
        if let Some(tags) = t.tags.as_deref() {
            if tags.contains(TEST_TAG) {
                let hash = t.hash.as_ref().unwrap();
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

    for old_hash in &old_hashes {
        assert!(
            !final_torrents_map.contains_key(old_hash),
            "Старый хеш НЕ удален!"
        );
    }

    for old_id in &old_ids {
        if let Some(new_hash) = final_ids_map.get(old_id) {
            let new_torrent_info = final_torrents_map.get(new_hash).unwrap();

            assert_eq!(
                new_torrent_info.category.as_deref(),
                Some(TEST_CATEGORY),
                "Категория НЕ сохранилась"
            );
            assert_eq!(
                new_torrent_info.tags.as_deref(),
                Some(multi_tags.as_str()),
                "Теги НЕ сохранились"
            );
        } else {
            let expected_path_str = if *old_id == id_1 {
                SAVE_PATH_1
            } else {
                SAVE_PATH_2
            };
            assert!(
                !std::path::Path::new(expected_path_str).exists(),
                "Директория НЕ удалена"
            );
        }
    }

    log::info!("--- 4. Фаза Очистки ---");
    cleanup_test_torrents(&client, vec![SAVE_PATH_1, SAVE_PATH_2]).await;
}

#[tokio::test]
#[ignore]
async fn test_dry_run_scenario() {
    if !Path::new(TORRENT_FILE_1).exists() {
        panic!("Тестовый файл не найден.");
    }
    setup_logger();

    let client = setup_client().await;
    let mut config = setup_config();
    config.dry_run = true;

    log::info!("--- 1. Начальная очистка ---");
    cleanup_test_torrents(&client, vec![SAVE_PATH_1]).await;

    log::info!("--- 1. Фаза Настройки ---");
    let (old_hash_1, _id_1) = add_test_torrent(
        &client,
        TORRENT_FILE_1,
        SAVE_PATH_1,
        TEST_CATEGORY,
        TEST_TAG,
    )
    .await
    .unwrap();

    log::info!("--- 2. Фаза Выполнения ---");
    let result = run_helper(config).await;
    assert!(result.is_ok());

    log::info!("--- 3. Фаза Проверки ---");
    let final_torrents = client.get_torrent_list(Default::default()).await.unwrap();
    let mut final_hashes_map = HashMap::new();

    for t in &final_torrents {
        if let Some(tags) = t.tags.as_deref() {
            if tags.contains(TEST_TAG) {
                let hash = t.hash.as_ref().unwrap();
                let props = client.get_torrent_properties(hash).await.unwrap();
                let id = rutracker_api::extract_torrent_id_from_comment(
                    &props.comment.unwrap_or_default(),
                );
                final_hashes_map.insert(hash.clone(), id);
            }
        }
    }

    assert_eq!(final_hashes_map.len(), 1, "Ожидался 1 торрент");
    assert!(
        final_hashes_map.contains_key(&old_hash_1),
        "Старый хеш отсутствует"
    );

    log::info!("--- 4. Фаза Очистки ---");
    cleanup_test_torrents(&client, vec![SAVE_PATH_1]).await;
}
