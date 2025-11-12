// src/rutracker_api.rs

use crate::torrent; // <-- Импортируем нашу структуру Torrent
use reqwest::header::{self, HeaderMap};
use reqwest::Client;
use serde::Deserialize;
use std::collections::HashMap;
use std::fmt;

use tokio::io::AsyncWriteExt; // <-- Импортируем для асинхронной записи в файл

// --- КОНСТАНТЫ API ---
const API_LIMIT_URL: &str = "https://api.rutracker.cc/v1/get_limit";
const API_PEER_STATS_URL: &str = "https://api.rutracker.cc/v1/get_peer_stats?by=hash&val=";
const API_TOR_HASH_URL: &str = "https://api.rutracker.cc/v1/get_tor_hash?by=topic_id&val=";

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
pub async fn get_api_limit_async() -> Result<u32, Box<dyn std::error::Error>> {
    let response_data: ApiResponseLimit = reqwest::get(API_LIMIT_URL)
        .await?
        .json::<ApiResponseLimit>()
        .await?;

    Ok(response_data.result.limit)
}

/// Вспомогательная функция для извлечения ID торрента из строки комментария (ищет 't=12345')
pub fn extract_torrent_id_from_comment(comment: &str) -> String {
    comment
        .rsplit_once("t=")
        .map(|(_, id)| id.to_string())
        .unwrap_or_default()
}

// --- СТРУКТУРЫ ДЛЯ get_peer_stats ---

// [seeders, leechers, seeder_last_seen, [keepers]]
#[derive(Deserialize, Debug)]
struct PeerStats(u32, u32, i64, Vec<u32>);

#[derive(Deserialize, Debug)]
struct ApiResponsePeerStats {
    // Значение Option<PeerStats> для обработки `null`
    result: std::collections::HashMap<String, Option<PeerStats>>,
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

// "Умный" enum, который может быть либо успехом, либо ошибкой
#[derive(Deserialize, Debug)]
#[serde(untagged)]
enum ApiResponse {
    Success(ApiResponsePeerStats),
    Error(ApiErrorResponse),
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
) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let limit = match get_api_limit_async().await {
        Ok(lim) => lim as usize,
        Err(e) => {
            log::warn!("⚠️ Не удалось получить лимит API, используем 20: {}", e);
            20
        }
    };

    let client = reqwest::Client::new();
    const MAX_ATTEMPTS_PER_CHUNK: u8 = 5;

    // (ИЗМЕНЕНО) Вектор для сбора ID проблемных торрентов
    let mut problematic_ids: Vec<String> = Vec::new();

    log::debug!("Обновление статистики порциями по {} торрентов...", limit);

    for torrent_chunk in my_torrents.chunks_mut(limit) {
        let mut current_work_list: Vec<&mut torrent::Torrent> = torrent_chunk.iter_mut().collect();
        let mut attempts = 0;

        'retry_loop: while !current_work_list.is_empty() && attempts < MAX_ATTEMPTS_PER_CHUNK {
            attempts += 1;

            log::debug!("Попытка {}, массив:{}", &attempts, current_work_list.len());

            let hashes: Vec<&str> = current_work_list
                .iter()
                .map(|t| t.torrent_hash.as_str())
                .collect();

            let hash_string = hashes.join(",");
            let url = format!("{}{}", API_PEER_STATS_URL, hash_string);

            match client.get(&url).send().await {
                Ok(response) => {
                    match response.json::<ApiResponse>().await {
                        // СЛУЧАЙ 1: УСПЕХ (включая `null` значения)
                        Ok(ApiResponse::Success(response_data)) => {
                            for torrent in current_work_list.iter_mut() {
                                if let Some(maybe_stats) =
                                    response_data.result.get(&torrent.torrent_hash)
                                {
                                    match maybe_stats {
                                        Some(stats) => {
                                            torrent.seeders = stats.0;
                                            torrent.leechers = stats.1;
                                            _ = stats.2;
                                            _ = &stats.3;
                                        }
                                        None => {
                                            // Хэш не найден, собираем ID
                                            log::warn!("⚠️ Хэш не найден на Rutracker (получен 'null'): {} (Торрент: {}, id:{})",
                                            torrent.torrent_hash, torrent.name, torrent.torrent_id);

                                            if !torrent.torrent_id.is_empty() {
                                                problematic_ids.push(torrent.torrent_id.clone());
                                            } else {
                                                log::warn!("⚠️ Торрент '{}' (хеш: {}) не найден на Rutracker, но у него нет ID в комментарии.", torrent.name, torrent.torrent_hash);
                                            }
                                        }
                                    }
                                } else {
                                    log::warn!(
                                        "⚠️ Хэш {} не был возвращен в ответе API (Торрент: {})",
                                        torrent.torrent_hash,
                                        torrent.name
                                    );
                                }
                            }
                            break 'retry_loop;
                        }

                        // СЛУЧАЙ 2: ОШИБКА API (ловим "Invalid hash")
                        Ok(ApiResponse::Error(error_data)) => {
                            log::warn!("⚠️ Ошибка API Rutracker: {}", error_data.error.text);

                            if let Some(bad_hash) = extract_invalid_hash(&error_data.error.text) {
                                log::warn!(
                                    "⚠️ Найден неверный хэш: {}. Повторяем запрос без него.",
                                    bad_hash
                                );

                                if let Some(index) = current_work_list
                                    .iter()
                                    .position(|t| t.torrent_hash == bad_hash)
                                {
                                    let removed_torrent = current_work_list.remove(index);
                                    log::warn!("⚠️ Торрент '{}' ({}) помечен как недействительный на Rutracker.", removed_torrent.name, bad_hash);

                                    // (ИЗМЕНЕНО) Собираем ID недействительного торрента
                                    if !removed_torrent.torrent_id.is_empty() {
                                        problematic_ids.push(removed_torrent.torrent_id.clone());
                                    } else {
                                        log::warn!("⚠️  Торрент '{}' (хеш: {}) недействителен на Rutracker, но у него нет ID в комментарии.", removed_torrent.name, bad_hash);
                                    }
                                } else {
                                    log::error!(
                                        "❌ Не удалось найти хэш {} в текущей пачке (уже удален?).",
                                        bad_hash
                                    );
                                    break 'retry_loop;
                                }
                            } else {
                                log::error!("❌ Не удалось исправить ошибку API. Пропускаем {} торрентов в этой пачке.", current_work_list.len());
                                break 'retry_loop;
                            }
                        }

                        // СЛУЧАЙ 3: ОШИБКА ДЕСЕРИАЛИЗАЦИИ
                        Err(e) => {
                            log::error!(
                                "❌ Ошибка десериализации JSON для пачки (url: {}): {}",
                                url,
                                e
                            );
                            break 'retry_loop;
                        }
                    }
                }
                // СЛУЧАЙ 4: ОШИБКА HTTP
                Err(e) => {
                    log::error!("❌ Ошибка HTTP запроса для пачки (url: {}): {}", url, e);
                    break 'retry_loop;
                }
            }
        }

        if attempts >= MAX_ATTEMPTS_PER_CHUNK && !current_work_list.is_empty() {
            log::error!(
                "❌ Достигнут лимит попыток ({}) для пачки. Пропускаем {} оставшихся торрентов.",
                MAX_ATTEMPTS_PER_CHUNK,
                current_work_list.len()
            );
        }
    }

    Ok(problematic_ids) // Возвращаем собранный вектор ID
}

// CТРУКТУРЫ ДЛЯ get_tor_hash ---

#[derive(Deserialize, Debug)]
struct ApiResponseTorHash {
    // Ключ - это ID (String), Значение - Option<String> (хэш или null)
    result: HashMap<String, Option<String>>,
}

// get_api_torrent_hash_by_id_async ---

/// Получает хэши торрентов по их ID с API Rutracker.
///
/// Принимает срез ID (которые могут быть представлены как &str, String, u32 и т.д.),
/// запрашивает API порциями и возвращает `HashMap<String, Option<String>>`,
/// где ключ - это ID, а значение - хэш или `None`, если ID не найден.
pub async fn get_api_torrent_hash_by_id_async<T>(
    ids: &[T],
) -> Result<HashMap<String, Option<String>>, Box<dyn std::error::Error>>
where
    T: fmt::Display, // <-- ИЗМЕНЕНИЕ: AsRef<str> заменен на fmt::Display
{
    // 1. Получаем лимит API
    let limit = match get_api_limit_async().await {
        Ok(lim) => lim as usize,
        Err(e) => {
            log::warn!(
                "⚠️ Не удалось получить лимит API для get_tor_hash, используем 20: {}",
                e
            );
            20
        }
    };

    let client = reqwest::Client::new();
    let mut all_results: HashMap<String, Option<String>> = HashMap::new();

    // (Добавлено) Пропускаем логирование, если массив пуст, и выходим
    if ids.is_empty() {
        return Ok(all_results);
    }

    log::debug!(
        "Запрос хэшей для {} ID, порциями по {}...",
        ids.len(),
        limit
    );

    // 2. Итерируем по ID порциями (чанками)
    for id_chunk in ids.chunks(limit) {
        // 3. Формируем строку ID через запятую
        let id_string = id_chunk
            .iter()
            .map(|id| id.to_string()) // <-- ИЗМЕНЕНИЕ: .as_ref() заменен на .to_string()
            .collect::<Vec<String>>() // <-- ИЗМЕНЕНИЕ: Vec<&str> на Vec<String>
            .join(",");

        let url = format!("{}{}", API_TOR_HASH_URL, id_string);
        log::debug!("  Запрос для IDs: [{}]", id_string);

        // 4. Выполняем запрос
        match client.get(&url).send().await {
            Ok(response) => {
                // 5. Десериализуем ответ
                match response.json::<ApiResponseTorHash>().await {
                    Ok(response_data) => {
                        // 6. Добавляем результаты из этой порции в общий HashMap
                        all_results.extend(response_data.result);
                    }
                    Err(e) => {
                        log::error!(
                            "❌ Ошибка десериализации JSON для get_tor_hash (url: {}): {}",
                            url,
                            e
                        );
                        // Пропускаем эту порцию
                    }
                }
            }
            Err(e) => {
                log::error!(
                    "❌ Ошибка HTTP запроса для get_tor_hash (url: {}): {}",
                    url,
                    e
                );
                // Пропускаем эту порцию
            }
        }
    }

    log::debug!(
        "Запрос хэшей завершен. Получено {} ответов.",
        all_results.len()
    );
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
) -> Result<String, Box<dyn std::error::Error>> {
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
        Err(format!(
            "Ошибка скачивания: сервер не вернул .torrent файл (статус: {}).",
            download_response.status()
        )
        .into())
    }
}
