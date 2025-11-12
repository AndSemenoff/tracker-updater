// tests/test_torrent.rs

// Импортируем структуру Torrent из нашего крейта (библиотеки)
use tracker_updater::torrent::Torrent;

#[test]
fn test_torrent_display_full() {
    let torrent = Torrent {
        name: "My Test Torrent".to_string(),
        torrent_hash: "aabbcc112233".to_string(),
        torrent_id: "98765".to_string(),
        tracker: "http://example-tracker.com".to_string(),
        comment: "Test comment t=98765".to_string(),
        state: "Downloading".to_string(),
        category: "Movies".to_string(),
        tags: "HD, Action".to_string(),
        size: 524288000, // 500 MB (500 * 1024 * 1024)
        seeders: 10,
        leechers: 2,
        save_path: "/downloads/movies".to_string(),
    };

    // Генерируем строку с помощью `format!`
    let display_str = format!("{}", torrent);

    // Проверяем наличие всех ключевых полей
    assert!(display_str.contains("Имя: My Test Torrent"));
    assert!(display_str.contains("ID: 98765"));
    assert!(display_str.contains("Хеш: aabbcc112233"));
    assert!(display_str.contains("Статус: Downloading"));
    assert!(display_str.contains("Категория: Movies"));
    assert!(display_str.contains("Теги: HD, Action"));
    assert!(display_str.contains("Размер: 500 MB")); // 524288000 / (1024*1024) = 500
    assert!(display_str.contains("Сиды: 10 | Личи: 2"));
    assert!(display_str.contains("Трекер: http://example-tracker.com"));
    assert!(display_str.contains("Комментарий: Test comment t=98765"));
    assert!(display_str.contains("Путь: /downloads/movies"));
}

#[test]
fn test_torrent_display_minimal() {
    // Тест для проверки, что пустые поля (трекер, коммент) не отображаются
    let torrent = Torrent {
        name: "Minimal Torrent".to_string(),
        torrent_hash: "min456".to_string(),
        torrent_id: "123".to_string(),
        tracker: "".to_string(), // Пусто
        comment: "".to_string(), // Пусто
        state: "Paused".to_string(),
        tags: "".to_string(),
        category: "Music".to_string(),
        size: 0,
        seeders: 0,
        leechers: 0,
        save_path: "".to_string(), // Пусто
    };

    let display_str = format!("{}", torrent);

    // Проверяем, что основные поля есть
    assert!(display_str.contains("Имя: Minimal Torrent"));
    assert!(display_str.contains("Статус: Paused"));
    assert!(display_str.contains("Размер: 0 MB"));

    // Важно: проверяем, что заголовки "Трекер:" и "Комментарий:" отсутствуют
    assert!(!display_str.contains("Трекер:"));
    assert!(!display_str.contains("Комментарий:"));
    assert!(!display_str.contains("Путь:"));
    assert!(!display_str.contains("Теги:"));
}

#[test]
fn test_torrent_debug_compact() {
    let torrent = Torrent {
        name: "My Test Torrent".to_string(),
        torrent_hash: "aabbcc112233".to_string(),
        torrent_id: "98765".to_string(),
        tracker: "http://example-tracker.com".to_string(),
        comment: "Test comment t=98765".to_string(),
        state: "Downloading".to_string(),
        category: "Movies".to_string(),
        tags: "HD, Action".to_string(),
        size: 524288000, // 500 MB (500 * 1024 * 1024)
        seeders: 10,
        leechers: 2,
        save_path: "/downloads/movies".to_string(),
    };

    // Генерируем строку с помощью `format!` и `{:?}`
    let debug_str = format!("{:?}", torrent);

    println!("Debug output:\n{}", debug_str);

    // 1. Проверяем, что это одна строка (нет переносов)
    assert!(
        !debug_str.contains('\n'),
        "Вывод Debug не должен содержать переносов строк"
    );

    // 2. Проверяем начало и конец
    assert!(debug_str.starts_with("Torrent {"));
    assert!(debug_str.ends_with("}"));

    // 3. Проверяем наличие ключевых полей
    assert!(debug_str.contains("name: \"My Test Torrent\""));
    assert!(debug_str.contains("id: \"98765\""));
    assert!(debug_str.contains("hash: \"aabbcc112233\""));
    assert!(debug_str.contains("state: \"Downloading\""));

    // 4. Проверяем наши кастомные поля
    assert!(debug_str.contains("peers: \"10/2\""));
    assert!(debug_str.contains("size_mb: 500"));
    assert!(debug_str.contains("tags: \"HD, Action\""));
    assert!(debug_str.contains("category: \"Movies\""));
}
