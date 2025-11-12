// tests/test_rutracker_api.rs

// Импортируем публичные функции из нашего крейта
use tracker_updater::rutracker_api::{
    extract_torrent_id_from_comment, //
    get_api_limit_async,
    get_api_torrent_hash_by_id_async,
};

// --- Тесты для extract_torrent_id_from_comment ---

#[test]
fn test_extract_id_happy_path() {
    let comment = "Какой-то текст https://rutracker.org/forum/viewtopic.php?t=1234567";
    assert_eq!(extract_torrent_id_from_comment(comment), "1234567");
}

#[test]
fn test_extract_id_no_id() {
    let comment = "Здесь нет ID";
    assert_eq!(extract_torrent_id_from_comment(comment), "");
}

#[test]
fn test_extract_id_empty_string() {
    let comment = "";
    assert_eq!(extract_torrent_id_from_comment(comment), "");
}

#[test]
fn test_extract_id_just_t_equals() {
    let comment = "t=";
    assert_eq!(extract_torrent_id_from_comment(comment), "");
}

#[test]
fn test_extract_id_multiple_t() {
    // Должен взять последнее вхождение
    let comment = "t=111 а потом t=222";
    assert_eq!(extract_torrent_id_from_comment(comment), "222");
}

// --- Интеграционный тест для get_api_limit_async ---

#[tokio::test]
async fn test_get_api_limit() {
    // Вызываем асинхронную функцию
    let result = get_api_limit_async().await;

    // 1. Проверяем, что запрос завершился успешно (Result::Ok)
    assert!(result.is_ok(), "Запрос к API не удался: {:?}", result.err());

    // 2. Распаковываем результат и проверяем, что лимит > 0
    let limit = result.unwrap();
    assert!(limit > 0, "Лимит API должен быть положительным числом");
}

// --- (НОВЫЙ БЛОК) Интеграционные тесты для get_api_torrent_hash_by_id_async ---

#[tokio::test]
async fn test_get_api_torrent_hash_by_id_basic() {
    // 1. Определяем ID для запроса, используя ваши данные
    // Мы используем &str, так как функция дженерик
    let ids_to_test = vec!["1", "2142"];

    // 2. Вызываем асинхронную функцию

    let result = get_api_torrent_hash_by_id_async(&ids_to_test).await;
    // 3. Проверяем, что запрос завершился успешно (Result::Ok)
    assert!(
        result.is_ok(),
        "Запрос get_tor_hash не удался: {:?}",
        result.err()
    );

    // 4. Распаковываем результат (HashMap)
    let hashes_map = result.unwrap();

    // 5. Проверяем, что карта содержит 2 элемента
    assert_eq!(
        hashes_map.len(),
        2,
        "Ожидалось 2 результата, получено {}",
        hashes_map.len()
    );

    // 6. Проверяем ID "1" (ожидаем null/None)
    // API возвращает null, что serde парсит в Option::None
    assert_eq!(
        hashes_map.get("1"),
        Some(&None),
        "ID 1 должен иметь значение None (null)"
    );

    // 7. Проверяем ID "2142" (ожидаем хэш)
    // API возвращает хэш, что serde парсит в Option::Some(String)
    let expected_hash = "658EDAB6AF0B424E62FEFEC0E39DBE2AC55B9AE3".to_string();
    assert_eq!(
        hashes_map.get("2142"),
        Some(&Some(expected_hash)),
        "ID 2142 имеет неверный хэш"
    );
}

#[tokio::test]
async fn test_get_api_torrent_hash_by_id_empty_and_types() {
    // 1. Тест с пустым вектором
    let empty_ids: Vec<u32> = vec![];
    let result_empty = get_api_torrent_hash_by_id_async(&empty_ids).await;

    assert!(result_empty.is_ok(), "Запрос с пустым вектором не удался");
    assert!(
        result_empty.unwrap().is_empty(),
        "Результат для пустого вектора должен быть пустым HashMap"
    );

    // 2. Тест с u32 (проверка дженерика <T>)
    let u32_ids = vec![2142];
    let result_u32 = get_api_torrent_hash_by_id_async(&u32_ids).await;
    assert!(result_u32.is_ok());

    let map_u32 = result_u32.unwrap();
    let expected_hash = "658EDAB6AF0B424E62FEFEC0E39DBE2AC55B9AE3".to_string();

    // Ключ в HashMap всегда String, даже если на входе u32
    assert_eq!(
        map_u32.get("2142"),
        Some(&Some(expected_hash)),
        "Ключ '2142' (из u32) не найден или хэш неверен"
    );
}
