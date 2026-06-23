#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
use arboard::Clipboard;
use enigo::{Enigo, Key, Keyboard, Settings};
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState, hotkey::{HotKey, Code}};
use tao::event_loop::{ControlFlow, EventLoopBuilder};
use tray_icon::{
    menu::{Menu, MenuItem, MenuEvent, CheckMenuItem},
    TrayIconBuilder, TrayIconEvent, Icon, MouseButton, MouseButtonState,
};
use winreg::{RegKey, enums::*};
use std::{fs, thread, time};
use std::io::Write;

// === ІМПОРТИ ДЛЯ ХУКУ ===
use rdev::{listen, Event as RdevEvent, EventType, Key as RdevKey};
use once_cell::sync::Lazy;
use std::sync::Mutex;

// === ІМПОРТИ ДЛЯ БЕЗШУМНОГО ПЕРЕМИКАННЯ WINAPI ===
use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId, SendMessageW, WM_INPUTLANGCHANGEREQUEST};
use windows::Win32::UI::Input::KeyboardAndMouse::{ActivateKeyboardLayout, GetKeyboardLayout, GetKeyboardLayoutList, HKL, KLF_SETFORPROCESS};
use windows::Win32::Foundation::{LPARAM, WPARAM};

// === ІМПОРТИ ДЛЯ ОДНОГО ЕКЗЕМПЛЯРУ ТА ЗВУКУ ===
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::Foundation::GetLastError;
use windows::Win32::Media::Audio::{PlaySoundW, SND_MEMORY, SND_ASYNC};

// === ГЛОБАЛЬНИЙ СТАН ===
static TYPED_BUFFER: Lazy<Mutex<String>> = Lazy::new(|| Mutex::new(String::new()));
static IS_CONVERTING: Lazy<Mutex<bool>> = Lazy::new(|| Mutex::new(false));
static SHIFT_PRESSED: Lazy<Mutex<bool>> = Lazy::new(|| Mutex::new(false));
static LAST_ACTIVE_HWND: Lazy<Mutex<isize>> = Lazy::new(|| Mutex::new(0));

const MAX_BUFFER_LEN: usize = 100;
// =========================

// ВИПРАВЛЕНО: Додано serde(default), щоб старі конфіги не ламали програму
#[derive(serde::Serialize, serde::Deserialize)]
struct Config {
    #[serde(default = "default_lang")]
    current_lang: String,
    #[serde(default)]
    auto_start: bool,
    #[serde(default)]
    remap_capslock: bool,
    #[serde(default = "default_sound_enabled")]
    sound_enabled: bool,
}

fn default_lang() -> String { "EN".into() }
fn default_sound_enabled() -> bool { true }

const APP_NAME: &str = "UK Layout Switcher";
const APP_VERSION: &str = env!("CARGO_PKG_VERSION");

const SWITCH_SOUND_BYTES: &[u8] = include_bytes!("../assets/switch.wav");

// === ФУНКЦІЯ ХУКУ КЛАВІАТУРИ ===
fn keyboard_callback(event: RdevEvent) {
    if *IS_CONVERTING.lock().unwrap() {
        return;
    }

    match event.event_type {
        EventType::KeyPress(key) => {
            let mut buffer = TYPED_BUFFER.lock().unwrap();
            let shift = *SHIFT_PRESSED.lock().unwrap();

            match key {
                RdevKey::ShiftLeft | RdevKey::ShiftRight => {
                    *SHIFT_PRESSED.lock().unwrap() = true;
                }
                RdevKey::Space => buffer.push(' '),
                RdevKey::Backspace => { buffer.pop(); }
                RdevKey::Return | RdevKey::Escape => buffer.clear(),
                
                RdevKey::KeyA => buffer.push(if shift { 'A' } else { 'a' }),
                RdevKey::KeyB => buffer.push(if shift { 'B' } else { 'b' }),
                RdevKey::KeyC => buffer.push(if shift { 'C' } else { 'c' }),
                RdevKey::KeyD => buffer.push(if shift { 'D' } else { 'd' }),
                RdevKey::KeyE => buffer.push(if shift { 'E' } else { 'e' }),
                RdevKey::KeyF => buffer.push(if shift { 'F' } else { 'f' }),
                RdevKey::KeyG => buffer.push(if shift { 'G' } else { 'g' }),
                RdevKey::KeyH => buffer.push(if shift { 'H' } else { 'h' }),
                RdevKey::KeyI => buffer.push(if shift { 'I' } else { 'i' }),
                RdevKey::KeyJ => buffer.push(if shift { 'J' } else { 'j' }),
                RdevKey::KeyK => buffer.push(if shift { 'K' } else { 'k' }),
                RdevKey::KeyL => buffer.push(if shift { 'L' } else { 'l' }),
                RdevKey::KeyM => buffer.push(if shift { 'M' } else { 'm' }),
                RdevKey::KeyN => buffer.push(if shift { 'N' } else { 'n' }),
                RdevKey::KeyO => buffer.push(if shift { 'O' } else { 'o' }),
                RdevKey::KeyP => buffer.push(if shift { 'P' } else { 'p' }),
                RdevKey::KeyQ => buffer.push(if shift { 'Q' } else { 'q' }),
                RdevKey::KeyR => buffer.push(if shift { 'R' } else { 'r' }),
                RdevKey::KeyS => buffer.push(if shift { 'S' } else { 's' }),
                RdevKey::KeyT => buffer.push(if shift { 'T' } else { 't' }),
                RdevKey::KeyU => buffer.push(if shift { 'U' } else { 'u' }),
                RdevKey::KeyV => buffer.push(if shift { 'V' } else { 'v' }),
                RdevKey::KeyW => buffer.push(if shift { 'W' } else { 'w' }),
                RdevKey::KeyX => buffer.push(if shift { 'X' } else { 'x' }),
                RdevKey::KeyY => buffer.push(if shift { 'Y' } else { 'y' }),
                RdevKey::KeyZ => buffer.push(if shift { 'Z' } else { 'z' }),
                
                RdevKey::Comma => buffer.push(if shift { '<' } else { ',' }),
                RdevKey::Dot => buffer.push(if shift { '>' } else { '.' }),
                RdevKey::SemiColon => buffer.push(if shift { ':' } else { ';' }),
                RdevKey::Quote => buffer.push(if shift { '"' } else { '\'' }),
                RdevKey::LeftBracket => buffer.push(if shift { '{' } else { '[' }),
                RdevKey::RightBracket => buffer.push(if shift { '}' } else { ']' }),

                RdevKey::Tab | RdevKey::LeftArrow | RdevKey::RightArrow | RdevKey::UpArrow | RdevKey::DownArrow |
                RdevKey::Home | RdevKey::End | RdevKey::ControlLeft | RdevKey::ControlRight => {
                    buffer.clear();
                }
                _ => {}
            }

            if buffer.len() > MAX_BUFFER_LEN {
                let drain_count = buffer.len() - MAX_BUFFER_LEN;
                buffer.drain(..drain_count);
            }
        }
        EventType::KeyRelease(key) => {
            if key == RdevKey::ShiftLeft || key == RdevKey::ShiftRight {
                *SHIFT_PRESSED.lock().unwrap() = false;
            }
        }
        EventType::ButtonPress(_) => {
            TYPED_BUFFER.lock().unwrap().clear();
        }
        _ => {}
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // --- ПЕРЕВІРКА НА ОДИН ЕКЗЕМПЛЯР ---
    unsafe {
        let _mutex = CreateMutexW(None, false, windows::core::w!("Local\\UK_Switcher_Mutex"));
        if GetLastError().0 == 183 { // ERROR_ALREADY_EXISTS
            std::process::exit(0);
        }
    }

    std::thread::spawn(|| {
        if let Err(error) = listen(keyboard_callback) {
            log_error(&format!("Помилка слухача клавіатури: {:?}", error));
        }
    });

    let config_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
        .unwrap_or_else(|| dirs::config_dir().unwrap().join("uk-switcher"));
        
    let config_path = config_dir.join("config.json");

    let mut config: Config = if config_path.exists() {
        serde_json::from_slice(&fs::read(&config_path)?)
            .unwrap_or(Config { current_lang: "EN".into(), auto_start: false, remap_capslock: false, sound_enabled: true })
    } else {
        Config { current_lang: "EN".into(), auto_start: false, remap_capslock: false, sound_enabled: true }
    };

    config.remap_capslock = get_capslock_remap_status();

    if config.auto_start {
        if let Err(e) = set_auto_start(true) {
            log_error(&format!("Не вдалося застосувати автозапуск при старті: {e}"));
        }
    }

    let icon_en_bytes = include_bytes!("../assets/en_icon.png");
    let icon_ua_bytes = include_bytes!("../assets/ua_icon.png");

    let icon_en = load_tray_icon(icon_en_bytes);
    let icon_ua = load_tray_icon(icon_ua_bytes);

    let tray_menu = Menu::new();
    
    let title_item = MenuItem::new(format!("UK Layout Switcher v{}", APP_VERSION), false, None);
    let auto_start_item = CheckMenuItem::new("Запускати з Windows", true, config.auto_start, None);
    let sound_item = CheckMenuItem::new("Звук перемикання", true, config.sound_enabled, None);
    let remap_item = CheckMenuItem::new("Remap CapsLock -> F24 (Req. Admin)", true, config.remap_capslock, None);
    let quit_item = MenuItem::new("Вийти", true, None);

    tray_menu.append(&title_item)?;
    tray_menu.append(&auto_start_item)?;
    tray_menu.append(&sound_item)?;
    tray_menu.append(&remap_item)?;
    tray_menu.append(&quit_item)?;

    let mut tray = TrayIconBuilder::new()
        .with_menu(Box::new(tray_menu))
        .with_tooltip(&format!("UK Switcher — {}", config.current_lang))
        .with_icon(if config.current_lang == "EN" { icon_en.clone() } else { icon_ua.clone() })
        .build()?;

    let manager = GlobalHotKeyManager::new()?;
    let hotkey = HotKey::new(None, Code::F24);
    
    if let Err(e) = manager.register(hotkey) {
        log_error(&format!("Не вдалося зареєструвати F24: {e}"));
    }

    let quit_id = quit_item.id().clone();
    let auto_start_id = auto_start_item.id().clone();
    let sound_id = sound_item.id().clone();
    let remap_id = remap_item.id().clone();

    let hotkey_receiver = GlobalHotKeyEvent::receiver();
    let tray_receiver = TrayIconEvent::receiver();
    let menu_receiver = MenuEvent::receiver();

    const DEBOUNCE_MS: u64 = 300;
    let mut last_action = time::Instant::now() - time::Duration::from_millis(DEBOUNCE_MS + 10);
    let mut last_lang_check = time::Instant::now();

    let event_loop = EventLoopBuilder::new().build();
    
    // ВИПРАВЛЕНО: Зберігаємо manager у змінну, яка захоплюється замиканням, щоб її не видалило з пам'яті
    let _manager = manager;
    
    event_loop.run(move |_, _, control_flow| {
        *control_flow = ControlFlow::WaitUntil(time::Instant::now() + time::Duration::from_millis(30));
        let _ = &_manager; // Тримаємо менеджер у пам'яті

        if last_lang_check.elapsed() >= time::Duration::from_millis(500) {
            last_lang_check = time::Instant::now();
            
            let os_lang = get_current_os_lang();
            if os_lang != config.current_lang {
                config.current_lang = os_lang;
                let _ = tray.set_icon(Some(if config.current_lang == "EN" { icon_en.clone() } else { icon_ua.clone() }));
                let _ = tray.set_tooltip(Some(&format!("UK Switcher — {}", config.current_lang)));
            }

            unsafe {
                let current_hwnd = GetForegroundWindow().0 as isize;
                let mut last_hwnd = LAST_ACTIVE_HWND.lock().unwrap();
                if *last_hwnd == 0 { *last_hwnd = current_hwnd; } 
                else if *last_hwnd != current_hwnd {
                    TYPED_BUFFER.lock().unwrap().clear();
                    *last_hwnd = current_hwnd;
                }
            }
        }

        if let Ok(event) = hotkey_receiver.try_recv() {
            if event.id == hotkey.id() && event.state == HotKeyState::Pressed {
                if last_action.elapsed() >= time::Duration::from_millis(DEBOUNCE_MS) {
                    last_action = time::Instant::now();
                    *IS_CONVERTING.lock().unwrap() = true;
                    
                    if let Err(e) = toggle_and_convert(&mut config, &mut tray, &icon_en, &icon_ua) {
                        log_error(&format!("Помилка конвертації: {:?}", e));
                    }
                    
                    *IS_CONVERTING.lock().unwrap() = false;
                }
            }
        }

        if let Ok(event) = tray_receiver.try_recv() {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                if last_action.elapsed() >= time::Duration::from_millis(DEBOUNCE_MS) {
                    last_action = time::Instant::now();
                    *IS_CONVERTING.lock().unwrap() = true;
                    
                    if let Err(e) = toggle_layout_only(&mut config, &mut tray, &icon_en, &icon_ua) {
                        log_error(&format!("Помилка перемикання: {:?}", e));
                    }
                    
                    *IS_CONVERTING.lock().unwrap() = false;
                }
            }
        }

        if let Ok(event) = menu_receiver.try_recv() {
            if event.id == quit_id {
                let _ = tray.set_visible(false);
                std::process::exit(0);
            } else if event.id == auto_start_id {
                config.auto_start = !config.auto_start;
                let _ = auto_start_item.set_checked(config.auto_start);
                if let Err(e) = set_auto_start(config.auto_start) {
                    log_error(&format!("Не вдалося змінити автозапуск: {e}"));
                }
                let _ = save_config(&config);
            } else if event.id == sound_id {
                config.sound_enabled = !config.sound_enabled;
                let _ = sound_item.set_checked(config.sound_enabled);
                let _ = save_config(&config);
            } else if event.id == remap_id {
                let new_state = !config.remap_capslock;
                if let Err(e) = set_capslock_remap(new_state) {
                    log_error(&format!("Помилка зміни Scancode Map (потрібні права Адміна?): {e}"));
                    let _ = remap_item.set_checked(config.remap_capslock);
                } else {
                    config.remap_capslock = new_state;
                    let _ = remap_item.set_checked(config.remap_capslock);
                    let _ = save_config(&config);
                }
            }
        }
    });
}

// ====================== РЕЄСТР ======================

fn set_auto_start(enable: bool) -> Result<(), Box<dyn std::error::Error>> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let run_key = hkcu.open_subkey_with_flags(r"Software\Microsoft\Windows\CurrentVersion\Run", KEY_WRITE)?;
    let exe_path = std::env::current_exe()?;
    let exe_path_quoted = format!("\"{}\"", exe_path.to_string_lossy());
    if enable { run_key.set_value(APP_NAME, &exe_path_quoted)?; } 
    else { let _ = run_key.delete_value(APP_NAME); }
    Ok(())
}

fn set_capslock_remap(enable: bool) -> Result<(), Box<dyn std::error::Error>> {
    use windows::Win32::System::Registry::*;
    use windows::Win32::Foundation::ERROR_SUCCESS;
    use windows::core::PCWSTR;

    let key_path = windows::core::w!("SYSTEM\\CurrentControlSet\\Control\\Keyboard Layout");
    let value_name = windows::core::w!("Scancode Map");
    
    unsafe {
        let mut h_key = Default::default();
        
        let open_result = RegOpenKeyExW(
            HKEY_LOCAL_MACHINE,
            PCWSTR(key_path.as_ptr()),
            0,
            KEY_SET_VALUE,
            &mut h_key,
        );

        if open_result != ERROR_SUCCESS {
            return Err(format!("Не вдалося відкрити ключ реєстру (код {}: потрібні права Адміністратора?)", open_result.0).into());
        }

        if enable {
            let data: [u8; 20] = [
                0x00, 0x00, 0x00, 0x00, 
                0x00, 0x00, 0x00, 0x00, 
                0x02, 0x00, 0x00, 0x00, 
                0x76, 0x00, 0x3a, 0x00, 
                0x00, 0x00, 0x00, 0x00  
            ];

            let set_result = RegSetValueExW(
                h_key,
                PCWSTR(value_name.as_ptr()),
                0,
                REG_BINARY,
                Some(&data),
            );
            
            if set_result != ERROR_SUCCESS {
                let _ = RegCloseKey(h_key);
                return Err(format!("Не вдалося записати значення (код {})", set_result.0).into());
            }
        } else {
            let del_result = RegDeleteValueW(h_key, PCWSTR(value_name.as_ptr()));
            if del_result != ERROR_SUCCESS && del_result.0 != 2 {
                let _ = RegCloseKey(h_key);
                return Err(format!("Не вдалося видалити значення (код {})", del_result.0).into());
            }
        }

        let close_result = RegCloseKey(h_key);
        if close_result != ERROR_SUCCESS {
            return Err(format!("Не вдалося закрити ключ реєстру (код {})", close_result.0).into());
        }
    }
    Ok(())
}

fn log_error(msg: &str) {
    let timestamp = time::SystemTime::now().duration_since(time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let log_line = format!("[{timestamp}] {msg}\n");
    
    if let Ok(exe_path) = std::env::current_exe() {
        if let Some(parent) = exe_path.parent() {
            let log_path = parent.join("error.log");
            if let Ok(mut file) = fs::OpenOptions::new().create(true).append(true).open(&log_path) {
                let _ = file.write_all(log_line.as_bytes());
                return;
            }
        }
    }
    
    if let Some(config_dir) = dirs::config_dir() {
        let log_dir = config_dir.join("uk-switcher");
        let _ = fs::create_dir_all(&log_dir);
        let log_path = log_dir.join("error.log");
        if let Ok(mut file) = fs::OpenOptions::new().create(true).append(true).open(&log_path) {
            let _ = file.write_all(log_line.as_bytes());
            return;
        }
    }
    
    let log_path = std::env::temp_dir().join("uk_switcher_error.log");
    if let Ok(mut file) = fs::OpenOptions::new().create(true).append(true).open(&log_path) {
        let _ = file.write_all(log_line.as_bytes());
    }
}

// ====================== ОСНОВНІ ФУНКЦІЇ ======================

fn play_switch_sound(config: &Config) {
    if config.sound_enabled {
        unsafe {
            let _ = PlaySoundW(
                windows::core::PCWSTR(SWITCH_SOUND_BYTES.as_ptr() as *const _),
                None,
                SND_MEMORY | SND_ASYNC,
            );
        }
    }
}

fn toggle_layout_only(
    config: &mut Config,
    tray: &mut tray_icon::TrayIcon,
    icon_en: &Icon,
    icon_ua: &Icon,
) -> Result<(), Box<dyn std::error::Error>> {
    config.current_lang = get_current_os_lang();
    config.current_lang = if config.current_lang == "EN" { "UA" } else { "EN" }.to_string();
    switch_system_layout_silent(&config.current_lang);
    play_switch_sound(config);
    let _ = tray.set_icon(Some(if config.current_lang == "EN" { icon_en.clone() } else { icon_ua.clone() }));
    let _ = tray.set_tooltip(Some(&format!("UK Switcher — {}", config.current_lang)));
    let _ = save_config(config);
    Ok(())
}

fn toggle_and_convert(
    config: &mut Config,
    tray: &mut tray_icon::TrayIcon,
    icon_en: &Icon,
    icon_ua: &Icon,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut clipboard = Clipboard::new()?;
    let mut enigo = Enigo::new(&Settings::default())?;

    let last_word_data = get_last_word_from_buffer();
    TYPED_BUFFER.lock().unwrap().clear();

    let original_os_lang = get_current_os_lang();
    config.current_lang = if original_os_lang == "EN" { "UA" } else { "EN" }.to_string();

    if let Some((word, delete_count)) = last_word_data {
        if !word.is_empty() {
            let to_ua = original_os_lang == "EN";
            let mut converted = convert_text(&word, to_ua);

            let spaces_to_add = delete_count.saturating_sub(word.chars().count());
            for _ in 0..spaces_to_add {
                converted.push(' ');
            }

            let saved_clipboard = clipboard.get_text().ok().filter(|s| !s.is_empty());
            clipboard.set_text(&converted)?;
            thread::sleep(time::Duration::from_millis(10));
            thread::sleep(time::Duration::from_millis(20));

            for _ in 0..delete_count {
                enigo.key(Key::Backspace, enigo::Direction::Press)?;
                thread::sleep(time::Duration::from_millis(10));
                enigo.key(Key::Backspace, enigo::Direction::Release)?;
                thread::sleep(time::Duration::from_millis(10));
            }
            thread::sleep(time::Duration::from_millis(20));

            enigo.key(Key::Control, enigo::Direction::Press)?;
            thread::sleep(time::Duration::from_millis(10));
            enigo.key(Key::V, enigo::Direction::Press)?;
            thread::sleep(time::Duration::from_millis(5));
            enigo.key(Key::V, enigo::Direction::Release)?;
            thread::sleep(time::Duration::from_millis(5));
            enigo.key(Key::Control, enigo::Direction::Release)?;
            thread::sleep(time::Duration::from_millis(25));

            if let Some(saved) = saved_clipboard { let _ = clipboard.set_text(&saved); } else { let _ = clipboard.clear(); }
        }
    } else {
        let saved_clipboard = clipboard.get_text().ok().filter(|s| !s.is_empty());
        let _ = clipboard.clear();
        thread::sleep(time::Duration::from_millis(10));

        enigo.key(Key::Control, enigo::Direction::Press)?;
        enigo.key(Key::Insert, enigo::Direction::Click)?;
        enigo.key(Key::Control, enigo::Direction::Release)?;
        thread::sleep(time::Duration::from_millis(25));

        let copied_text = clipboard.get_text().unwrap_or_default();

        if !copied_text.is_empty() {
            let cleaned_text = copied_text.trim_end_matches(|c| c == '\r' || c == '\n').to_string();
            let to_ua = detect_conversion_direction(&cleaned_text);
            let converted = convert_text(&cleaned_text, to_ua);
            
            clipboard.set_text(&converted)?;
            enigo.key(Key::Control, enigo::Direction::Press)?;
            enigo.key(Key::V, enigo::Direction::Click)?;
            enigo.key(Key::Control, enigo::Direction::Release)?;
            thread::sleep(time::Duration::from_millis(25));
        }

        if let Some(saved) = saved_clipboard { let _ = clipboard.set_text(&saved); } else { let _ = clipboard.clear(); }
    }

    switch_system_layout_silent(&config.current_lang);
    play_switch_sound(config);
    let _ = tray.set_icon(Some(if config.current_lang == "EN" { icon_en.clone() } else { icon_ua.clone() }));
    let _ = tray.set_tooltip(Some(&format!("UK Switcher — {}", config.current_lang)));
    let _ = save_config(config);
    
    thread::sleep(time::Duration::from_millis(50));
    Ok(())
}

fn get_last_word_from_buffer() -> Option<(String, usize)> {
    let mut buffer = TYPED_BUFFER.lock().unwrap();
    if buffer.is_empty() {
        return None;
    }

    let original_chars = buffer.chars().count();

    let trimmed = buffer.trim_end();
    if trimmed.is_empty() {
        buffer.clear();
        return None;
    }

    let word_start = trimmed.rfind(' ').map(|i| i + 1).unwrap_or(0);
    let last_word = trimmed[word_start..].to_string();

    *buffer = buffer[..word_start].to_string();

    let delete_count = original_chars - buffer.chars().count();

    Some((last_word, delete_count))
}

fn get_current_os_lang() -> String {
    unsafe {
        let hwnd = GetForegroundWindow();
        let thread_id = GetWindowThreadProcessId(hwnd, None);
        let current_hkl = GetKeyboardLayout(thread_id);
        let lang_id = (current_hkl.0 as u32) & 0xFFFF;
        match lang_id { 0x0422 => "UA".to_string(), _ => "EN".to_string() }
    }
}

fn switch_system_layout_silent(target_lang: &str) {
    let target_lang_id = if target_lang == "UA" { 0x0422u32 } else { 0x0409u32 };
    unsafe {
        let hwnd = GetForegroundWindow();
        let count = GetKeyboardLayoutList(None);
        if count > 0 {
            let mut hkl_list = vec![HKL(std::ptr::null_mut()); count as usize];
            let actual_count = GetKeyboardLayoutList(Some(&mut hkl_list));
            for i in 0..actual_count as usize {
                let lang_id = (hkl_list[i].0 as u32) & 0xFFFF;
                if lang_id == target_lang_id {
                    SendMessageW(hwnd, WM_INPUTLANGCHANGEREQUEST, WPARAM(0), LPARAM(hkl_list[i].0 as isize));
                    let _ = ActivateKeyboardLayout(hkl_list[i], KLF_SETFORPROCESS);
                    break;
                }
            }
        }
    }
}

fn convert_text(text: &str, to_ua: bool) -> String {
    let en_to_ua = [
        ('q','й'), ('w','ц'), ('e','у'), ('r','к'), ('t','е'), ('y','н'), ('u','г'), ('i','ш'), ('o','щ'), ('p','з'),
        ('[','х'), (']','ї'), ('a','ф'), ('s','і'), ('d','в'), ('f','а'), ('g','п'), ('h','р'), ('j','о'), ('k','л'),
        ('l','д'), (';','ж'), ('\'','є'), ('z','я'), ('x','ч'), ('c','с'), ('v','м'), ('b','и'), ('n','т'), ('m','ь'),
        (',','б'), ('.','ю'), ('`','\''),
        ('Q','Й'), ('W','Ц'), ('E','У'), ('R','К'), ('T','Е'), ('Y','Н'), ('U','Г'), ('I','Ш'), ('O','Щ'), ('P','З'),
        ('{','Х'), ('}','Ї'), ('A','Ф'), ('S','І'), ('D','В'), ('F','А'), ('G','П'), ('H','Р'), ('J','О'), ('K','Л'),
        ('L','Д'), (':','Ж'), ('"','Є'), ('Z','Я'), ('X','Ч'), ('C','С'), ('V','М'), ('B','И'), ('N','Т'), ('M','Ь'),
        ('<','Б'), ('>','Ю'), ('~','\''),
    ];
    let ua_to_en = [
        ('й','q'), ('ц','w'), ('у','e'), ('к','r'), ('е','t'), ('н','y'), ('г','u'), ('ш','i'), ('щ','o'), ('з','p'),
        ('х','['), ('ї',']'), ('ф','a'), ('і','s'), ('в','d'), ('а','f'), ('п','g'), ('р','h'), ('о','j'), ('л','k'),
        ('д','l'), ('ж',';'), ('є','\''), ('я','z'), ('ч','x'), ('с','c'), ('м','v'), ('и','b'), ('т','n'), ('ь','m'),
        ('б',','), ('ю','.'), ('\'','`'),
        ('Й','Q'), ('Ц','W'), ('У','E'), ('К','R'), ('Е','T'), ('Н','Y'), ('Г','U'), ('Ш','I'), ('Щ','O'), ('З','P'),
        ('Х','{'), ('Ї','}'), ('Ф','A'), ('І','S'), ('В','D'), ('А','F'), ('П','G'), ('Р','H'), ('О','J'), ('Л','K'),
        ('Д','L'), ('Ж',':'), ('Є','"'), ('Я','Z'), ('Ч','X'), ('С','C'), ('М','V'), ('И','B'), ('Т','N'), ('Ь','M'),
        ('Б','<'), ('Ю','>'),
    ];
    let map = if to_ua { &en_to_ua[..] } else { &ua_to_en[..] };
    text.chars().map(|c| map.iter().find(|&&(from, _)| from == c).map(|&(_, to)| to).unwrap_or(c)).collect()
}

fn detect_conversion_direction(text: &str) -> bool {
    let mut latin = 0usize;
    let mut cyrillic = 0usize;
    for c in text.chars() {
        if c.is_ascii_alphabetic() { latin += 1; } 
        else if c.is_alphabetic() { cyrillic += 1; }
    }
    latin >= cyrillic
}

fn save_config(config: &Config) -> std::io::Result<()> {
    let path = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.join("config.json")))
        .unwrap_or_else(|| dirs::config_dir().unwrap().join("uk-switcher/config.json"));
    fs::write(path, serde_json::to_string_pretty(config)?)
}

fn load_tray_icon(bytes: &[u8]) -> Icon {
    let img = image::load_from_memory(bytes).expect("Не вдалося декодувати зображення");
    let rgba = img.to_rgba8();
    let (width, height) = rgba.dimensions();
    let pixels = rgba.into_raw();
    Icon::from_rgba(pixels, width, height).expect("Не вдалося створити іконку")
}

fn get_capslock_remap_status() -> bool {
    use windows::Win32::System::Registry::*;
    use windows::Win32::Foundation::ERROR_SUCCESS;
    use windows::core::PCWSTR;

    let key_path = windows::core::w!("SYSTEM\\CurrentControlSet\\Control\\Keyboard Layout");
    let value_name = windows::core::w!("Scancode Map");

    unsafe {
        let mut h_key = Default::default();
        
        let open_result = RegOpenKeyExW(
            HKEY_LOCAL_MACHINE,
            PCWSTR(key_path.as_ptr()),
            0,
            KEY_READ,
            &mut h_key,
        );

        if open_result != ERROR_SUCCESS {
            return false;
        }

        let mut data: [u8; 20] = [0; 20];
        let mut data_len: u32 = 20;

        let query_result = RegQueryValueExW(
            h_key,
            PCWSTR(value_name.as_ptr()),
            None,
            None,
            Some(data.as_mut_ptr()),
            Some(&mut data_len as *mut u32),
        );

        let _ = RegCloseKey(h_key);

        if query_result != ERROR_SUCCESS || data_len != 20 {
            return false; 
        }

        let expected_data: [u8; 20] = [
            0x00, 0x00, 0x00, 0x00, 
            0x00, 0x00, 0x00, 0x00, 
            0x02, 0x00, 0x00, 0x00, 
            0x76, 0x00, 0x3a, 0x00, 
            0x00, 0x00, 0x00, 0x00  
        ];

        data == expected_data
    }
}