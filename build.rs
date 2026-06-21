fn main() {
    // Вбудовуємо іконку тільки під час збірки під Windows
    if std::env::var("CARGO_CFG_TARGET_OS").unwrap() == "windows" {
        let mut res = winres::WindowsResource::new();
        // Вказуємо шлях до нашого .ico файлу
        res.set_icon("assets/app_icon.ico");
        res.compile().expect("Не вдалося скомпілювати ресурс іконки .ico");
    }
}