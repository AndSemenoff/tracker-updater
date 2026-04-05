// src/rutracker_api.rs

use crate::torrent;
use anyhow::{Context, Result};
use reqwest::header::{self, HeaderMap};
use reqwest::Client;
use serde::Deserialize;
use std::collections::HashMap;
use std::fmt;

use tokio::io::AsyncWriteExt;

// --- КОНСТАНТЫ API ---
const API_LIMIT_URL: &str = "https://api.rutracker.cc/v1/get_limit";
const API_TOR_TOPIC_DATA_URL: &str = "https://api.rutracker.cc/v1/get_tor_topic_data";

//const API_PEER_STATS_URL: &str = "https://api.rutracker.cc/v1/get_peer_stats?by=hash&val=";
//const API_TOR_HASH_URL: &str = "https://api.rutracker.cc/v1/get_tor_hash?by=topic_id&val=";

// --- СТРУКТУРЫ ДЛЯ get_limit ---
#[derive(Deserialize)]
struct ApiResponseLimit {
    result: ResultDataLimit,
}

#[derive(Deserialize)]
struct ResultDataLimit {
    limit: u32,
}

/// Вспомогательная функция для получения лимита API от rutracker.cc
/// Возвращает Result с числом лимита или ошибкой
pub async fn get_api_limit_async() -> Result<u32> {
    let response_data: ApiResponseLimit = reqwest::get(API_LIMIT_URL)
        .await
        .context("Ошибка при выполнении запроса к API лимитов")?
        .json::<ApiResponseLimit>()
        .await
        .context("Ошибка при десериализации ответа лимитов")?;

    Ok(response_data.result.limit)
}

/// Вспомогательная функция для извлечения ID торрента из строки комментария (ищет 't=12345')
pub fn extract_torrent_id_from_comment(comment: &str) -> String {
    comment
        .rsplit_once("t=")
        .map(|(_, id)| id.to_string())
        .unwrap_or_default()
}

#[derive(Deserialize, Debug)]
struct TopicData {
    info_hash: String,
    seeders: u32,
}

#[derive(Deserialize, Debug)]
struct ApiResponseTopicData {
    result: std::collections::HashMap<String, Option<TopicData>>,
}

#[derive(Deserialize, Debug)]
#[serde(untagged)]
enum ApiResponse {
    Success(ApiResponseTopicData),
    Error(ApiErrorResponse),
}

// Структуры для десериализации ответа get_peer_stats (ОШИБКА)
#[derive(Deserialize, Debug)]
struct ApiErrorDetail {
    #[allow(dead_code)]
    code: i32,
    text: String,
}

#[derive(Deserialize, Debug)]
struct ApiErrorResponse {
    error: ApiErrorDetail,
}

/// Вспомогательная функция для извлечения неверного хэша из текста ошибки
fn extract_invalid_hash(error_text: &str) -> Option<&str> {
    error_text
        .rsplit_once("Invalid hash: ")
        .map(|(_, hash)| hash.trim())
}

// --- ОСНОВНАЯ ФУНКЦИЯ API (get_peer_stats) ---

/// Обновляет статистику сидов/личей для вектора торрентов,
/// запрашивая данные у API Rutracker порциями.
///
/// Функция изменяет `my_torrents` по месту (in-place).
///
/// Возвращает `Result` с вектором ID торрентов,
/// которые не были найдены на Rutracker (null или invalid hash).
pub async fn get_api_peer_stats_by_hash_async(
    my_torrents: &mut [torrent::Torrent],
) -> Result<Vec<String>> {
    let limit = match get_api_limit_async().await {
        Ok(lim) => (lim as usize).min(50), // Принудительное ограничение до 50
        Err(e) => {
            log::warn!("⚠️ Не удалось получить лимит API, используем 20: {}", e);
            20
        }
    };

    let client = reqwest::Client::new();
    const MAX_ATTEMPTS_PER_CHUNK: u8 = 5;
    let mut problematic_ids: Vec<String> = Vec::new();

    log::debug!("Обновление статистики порциями по {} торрентов...", limit);

    for torrent_chunk in my_torrents.chunks_mut(limit) {
        let mut current_work_list: Vec<&mut torrent::Torrent> = torrent_chunk.iter_mut().collect();
        let mut attempts = 0;

        'retry_loop: while !current_work_list.is_empty() && attempts < MAX_ATTEMPTS_PER_CHUNK {
            attempts += 1;

            let hashes: Vec<&str> = current_work_list
                .iter()
                .map(|t| t.torrent_hash.as_str())
                .collect();

            let hash_string = hashes.join(",");
            let url = format!("{}?by=hash&val={}", API_TOR_TOPIC_DATA_URL, hash_string);

            match client.get(&url).send().await {
                Ok(response) => {
                    match response.json::<ApiResponse>().await {
                        Ok(ApiResponse::Success(response_data)) => {
                            // API возвращает ключи в виде ID, нужно сопоставить по info_hash
                            let mut hash_to_stats: HashMap<String, TopicData> = HashMap::new();
                            for (_, maybe_data) in response_data.result {
                                if let Some(data) = maybe_data {
                                    hash_to_stats.insert(data.info_hash.to_uppercase(), data);
                                }
                            }

                            for torrent in current_work_list.iter_mut() {
                                if let Some(stats) =
                                    hash_to_stats.get(&torrent.torrent_hash.to_uppercase())
                                {
                                    torrent.seeders = stats.seeders;
                                    torrent.leechers = 0; // В новом API нет личей
                                } else {
                                    log::warn!(
                                        "⚠️ Хэш не найден на Rutracker: {} (Торрент: {})",
                                        torrent.torrent_hash,
                                        torrent.name
                                    );
                                    if !torrent.torrent_id.is_empty() {
                                        problematic_ids.push(torrent.torrent_id.clone());
                                    }
                                }
                            }
                            break 'retry_loop;
                        }
                        Ok(ApiResponse::Error(error_data)) => {
                            log::warn!("⚠️ Ошибка API Rutracker: {}", error_data.error.text);
                            if let Some(bad_hash) = extract_invalid_hash(&error_data.error.text) {
                                if let Some(index) = current_work_list
                                    .iter()
                                    .position(|t| t.torrent_hash == bad_hash)
                                {
                                    let removed_torrent = current_work_list.remove(index);
                                    if !removed_torrent.torrent_id.is_empty() {
                                        problematic_ids.push(removed_torrent.torrent_id.clone());
                                    }
                                } else {
                                    break 'retry_loop;
                                }
                            } else {
                                break 'retry_loop;
                            }
                        }
                        Err(e) => {
                            log::error!("❌ Ошибка десериализации JSON: {}", e);
                            break 'retry_loop;
                        }
                    }
                }
                Err(e) => {
                    log::error!("❌ Ошибка HTTP запроса: {}", e);
                    break 'retry_loop;
                }
            }
        }
    }

    Ok(problematic_ids)
}

// CТРУКТУРЫ ДЛЯ get_tor_hash ---

//#[derive(Deserialize, Debug)]
//struct ApiResponseTorHash {
// Ключ - это ID (String), Значение - Option<String> (хэш или null)
//    result: HashMap<String, Option<String>>,
//}

// get_api_torrent_hash_by_id_async ---

/// Получает хэши торрентов по их ID с API Rutracker.
///
/// Принимает срез ID (которые могут быть представлены как &str, String, u32 и т.д.),
/// запрашивает API порциями и возвращает `HashMap<String, Option<String>>`,
/// где ключ - это ID, а значение - хэш или `None`, если ID не найден.
pub async fn get_api_torrent_hash_by_id_async<T>(
    ids: &[T],
) -> Result<HashMap<String, Option<String>>>
where
    T: fmt::Display,
{
    let limit = match get_api_limit_async().await {
        Ok(lim) => (lim as usize).min(50),
        Err(_) => 20,
    };

    let client = reqwest::Client::new();
    let mut all_results: HashMap<String, Option<String>> = HashMap::new();

    if ids.is_empty() {
        return Ok(all_results);
    }

    for id_chunk in ids.chunks(limit) {
        let id_string = id_chunk
            .iter()
            .map(|id| id.to_string())
            .collect::<Vec<String>>()
            .join(",");

        let url = format!("{}?by=topic_id&val={}", API_TOR_TOPIC_DATA_URL, id_string);

        match client.get(&url).send().await {
            Ok(response) => {
                // Используем ApiResponseTopicData напрямую
                match response.json::<ApiResponseTopicData>().await {
                    Ok(response_data) => {
                        for (id, maybe_data) in response_data.result {
                            all_results.insert(id, maybe_data.map(|d| d.info_hash));
                        }
                    }
                    Err(e) => log::error!("❌ Ошибка десериализации: {}", e),
                }
            }
            Err(e) => log::error!("❌ Ошибка HTTP: {}", e),
        }
    }

    Ok(all_results)
}

///
/// АСИНХРОННО скачивает .torrent файл по ID темы, используя существующий клиент и заголовки.
///
/// # Аргументы
/// * `client` - Ссылка на асинхронный reqwest::Client
/// * `headers` - Ссылка на HeaderMap с вашими cookie и User-Agent
/// * `topic_id` - ID темы (например, 6557126)
///
/// # Возвращает
/// * `Result<String, ...>` - Путь к скачанному файлу (например, "t12345.torrent")
// (ИЗМЕНЕНО) Добавлено 'pub' и изменен тип возврата
pub async fn download_torrent(
    client: &Client,
    headers: &HeaderMap,
    topic_id: u64,
) -> Result<String> {
    // 1. Формируем URL и имя файла динамически
    let download_url = format!("https://rutracker.org/forum/dl.php?t={}", topic_id);
    let output_filename = format!("t{}.torrent", topic_id);

    // 2. Пытаемся скачать файл
    log::info!("Попытка скачивания файла (тема {})...", topic_id);

    let mut download_response = client
        .get(&download_url)
        // Мы .clone() заголовки, чтобы они не "потратились"
        // и их можно было использовать в следующем вызове
        .headers(headers.clone())
        .send()
        .await?; // Ждем ответа

    // 3. Проверяем ответ (та же логика, что и была)
    let content_type = download_response
        .headers()
        .get(header::CONTENT_TYPE)
        .map(|v| v.to_str().unwrap_or(""))
        .unwrap_or("");

    log::debug!("Статус ответа: {}", download_response.status());
    log::debug!("Тип контента: {}", content_type);

    if download_response.status().is_success() && content_type.contains("application/x-bittorrent")
    {
        log::info!("✅ Успешная авторизация! Начинаю скачивание .torrent файла...");

        // Используем асинхронный File::create
        let mut output_file = tokio::fs::File::create(&output_filename).await?;

        // 4. ИСПРАВЛЕНИЕ: Читаем тело ответа по чанкам (кускам)
        while let Some(chunk) = download_response.chunk().await? {
            output_file.write_all(&chunk).await?;
        }

        log::debug!("✅ Файл '{}' успешно скачан.", output_filename);
        Ok(output_filename) // <-- (ИЗМЕНЕНО) Возвращаем путь к файлу
    } else {
        log::error!("❌ Ошибка авторизации или скачивания.");
        log::error!("Сервер не вернул .torrent файл. Вероятно, ваша 'bb_session' cookie устарела.");

        // Возвращаем новую ошибку, чтобы `main` мог ее обработать
        Err(anyhow::anyhow!(
            "Ошибка скачивания: сервер не вернул .torrent файл (статус: {}).",
            download_response.status()
        ))
    }
}
