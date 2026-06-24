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

// === ІМПОРТИ ДЛЯ РІДНОГО ХУКУ WINDOWS (замість rdev) ===
use once_cell::sync::Lazy;
use std::sync::Mutex;
use windows::Win32::UI::WindowsAndMessaging::{
    SetWindowsHookExW, UnhookWindowsHookEx, CallNextHookEx, GetMessageW, MSG,
    WH_KEYBOARD_LL, WH_MOUSE_LL, KBDLLHOOKSTRUCT, HC_ACTION, WM_KEYDOWN, WM_SYSKEYDOWN, 
    WM_LBUTTONDOWN, WM_RBUTTONDOWN, WM_MBUTTONDOWN
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_SHIFT};
use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};

// === ІМПОРТИ ДЛЯ БЕЗШУМНОГО ПЕРЕМИКАННЯ WINAPI ===
use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId, SendMessageW, WM_INPUTLANGCHANGEREQUEST};
use windows::Win32::UI::Input::KeyboardAndMouse::{ActivateKeyboardLayout, GetKeyboardLayout, GetKeyboardLayoutList, HKL, KLF_SETFORPROCESS};

// === ІМПОРТИ ДЛЯ ОДНОГО ЕКЗЕМПЛЯРУ ТА ЗВУКУ ===
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::Foundation::GetLastError;
use windows::Win32::Media::Audio::{PlaySoundW, SND_MEMORY, SND_ASYNC};

// === ГЛОБАЛЬНИЙ СТАН ===
static TYPED_BUFFER: Lazy<Mutex<String>> = Lazy::new(|| Mutex::new(String::new()));
static IS_CONVERTING: Lazy<Mutex<bool>> = Lazy::new(|| Mutex::new(false));
static LAST_ACTIVE_HWND: Lazy<Mutex<isize>> = Lazy::new(|| Mutex::new(0));

const MAX_BUFFER_LEN: usize = 100;
// =========================

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

// === ФУНКЦІЇ РІДНОГО ХУКУ ===
unsafe extern "system" fn keyboard_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 {
        let kb = &*(lparam.0 as *const KBDLLHOOKSTRUCT);
        let msg = wparam.0 as u32;
        
        if msg == WM_KEYDOWN || msg == WM_SYSKEYDOWN {
            if !*IS_CONVERTING.lock().unwrap() {
                let mut buffer = TYPED_BUFFER.lock().unwrap();
                let vk = kb.vkCode;
                
                // Перевіряємо Shift через WinAPI
                let shift = GetAsyncKeyState(VK_SHIFT.0 as i32) < 0;
                
                match vk {
                    0x08 => { buffer.pop(); } // Backspace
                    0x0D | 0x1B => buffer.clear(), // Return | Escape
                    0x20 => buffer.push(' '), // Space
                    0x09 => buffer.clear(), // Tab
                    0x25 | 0x26 | 0x27 | 0x28 => buffer.clear(), // Arrows
                    0x24 | 0x23 => buffer.clear(), // Home | End
                    
                    // A-Z
                    v if (0x41..=0x5A).contains(&v) => {
                        let c = (v as u8 as char).to_ascii_lowercase();
                        buffer.push(if shift { c.to_ascii_uppercase() } else { c });
                    }
                    
                    // 0-9 (верхній ряд)
                    v if (0x30..=0x39).contains(&v) => {
                        let idx = (v - 0x30) as usize;
                        let no_shift = ['0', '1', '2', '3', '4', '5', '6', '7', '8', '9'];
                        let with_shift = [')', '!', '@', '#', '$', '%', '^', '&', '*', '('];
                        buffer.push(if shift { with_shift[idx] } else { no_shift[idx] });
                    }
                    
                    // OEM Спеціальні символи
                    0xBA => buffer.push(if shift { ':' } else { ';' }),
                    0xBB => buffer.push(if shift { '+' } else { '=' }),
                    0xBC => buffer.push(if shift { '<' } else { ',' }),
                    0xBD => buffer.push(if shift { '_' } else { '-' }),
                    0xBE => buffer.push(if shift { '>' } else { '.' }),
                    0xBF => buffer.push(if shift { '?' } else { '/' }),
                    0xC0 => buffer.push(if shift { '~' } else { '`' }),
                    0xDB => buffer.push(if shift { '{' } else { '[' }),
                    0xDC => buffer.push(if shift { '|' } else { '\\' }),
                    0xDD => buffer.push(if shift { '}' } else { ']' }),
                    0xDE => buffer.push(if shift { '"' } else { '\'' }),
                    
                    // Numpad цифри
                    0x60..=0x69 => buffer.push(((vk - 0x60) as u8 + b'0') as char),
                    0x6A => buffer.push('*'),
                    0x6B => buffer.push('+'),
                    0x6D => buffer.push('-'),
                    0x6E => buffer.push('.'),
                    0x6F => buffer.push('/'),
                    
                    _ => {}
                }
                
                if buffer.len() > MAX_BUFFER_LEN {
                    let drain_count = buffer.len() - MAX_BUFFER_LEN;
                    buffer.drain(..drain_count);
                }
            }
        }
    }
    CallNextHookEx(None, code, wparam, lparam)
}

unsafe extern "system" fn mouse_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 {
        let msg = wparam.0 as u32;
        if msg == WM_LBUTTONDOWN || msg == WM_RBUTTONDOWN || msg == WM_MBUTTONDOWN {
            TYPED_BUFFER.lock().unwrap().clear();
        }
    }
    CallNextHookEx(None, code, wparam, lparam)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // --- ПЕРЕВІРКА НА ОДИН ЕКЗЕМПЛЯР ---
    unsafe {
        let _mutex = CreateMutexW(None, false, windows::core::w!("Local\\UK_Switcher_Mutex"));
        if GetLastError().0 == 183 {
            std::process::exit(0);
        }
    }

    // --- ІНІЦІАЛІЗАЦІЯ РІДНИХ ХУКІВ ---
        // --- ІНІЦІАЛІЗАЦІЯ РІДНИХ ХУКІВ ---
    std::thread::spawn(|| {
        unsafe {
            let h_inst = GetModuleHandleW(None).unwrap();
            let kb_hook = SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_proc), h_inst, 0);
            let mouse_hook = SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_proc), h_inst, 0);
            
            // ВИПРАВЛЕНО: перевірка помилок та розгортання Result
            if kb_hook.is_err() || mouse_hook.is_err() {
                log_error("Не вдалося встановити системні хуки.");
                return;
            }
            
            let kb_hook = kb_hook.unwrap();
            let mouse_hook = mouse_hook.unwrap();
            
            let mut msg = MSG::default();
            while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                // Цикл повідомлень тримає хуки активними
            }
            
            let _ = UnhookWindowsHookEx(kb_hook);
            let _ = UnhookWindowsHookEx(mouse_hook);
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
    let _manager = manager;
    
    event_loop.run(move |_, _, control_flow| {
        *control_flow = ControlFlow::WaitUntil(time::Instant::now() + time::Duration::from_millis(30));
        let _ = &_manager;

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

fn spawn_clipboard_restore(saved: Option<String>) {
    thread::spawn(move || {
        thread::sleep(time::Duration::from_millis(450));
        if let Ok(mut cb) = Clipboard::new() {
            match saved {
                Some(text) => { let _ = cb.set_text(&text); }
                None => { let _ = cb.clear(); }
            }
        }
    });
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

    // --- ДРУК БУФЕРУ В ЛОГ ---
    let buffer_content = TYPED_BUFFER.lock().unwrap().clone();
    log_error(&format!("--- НАТИСК CapsLock ---"));
    log_error(&format!("Вміст буфера: [{}]", buffer_content));

    let last_word_data = get_last_word_from_buffer();
    TYPED_BUFFER.lock().unwrap().clear();

    let original_os_lang = get_current_os_lang();
    config.current_lang = if original_os_lang == "EN" { "UA" } else { "EN" }.to_string();

    if let Some((word, delete_count)) = last_word_data {
        log_error(&format!("Знайдено слово: [{}], Видалити символів: {}", word, delete_count));
        
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

            spawn_clipboard_restore(saved_clipboard);
        }
    } else {
        log_error("Буфер порожній. Спроба конвертації виділеного тексту.");
        
        let marker = "___UK_SWITCHER_EMPTY_MARKER___";
        let saved_clipboard = clipboard.get_text().ok().filter(|s| !s.is_empty());
        
        let _ = clipboard.set_text(marker);
        thread::sleep(time::Duration::from_millis(10));

        enigo.key(Key::Control, enigo::Direction::Press)?;
        enigo.key(Key::Insert, enigo::Direction::Click)?;
        enigo.key(Key::Control, enigo::Direction::Release)?;
        thread::sleep(time::Duration::from_millis(25));

        let copied_text = clipboard.get_text().unwrap_or_default();

        if !copied_text.is_empty() && copied_text != marker {
            let cleaned_text = copied_text.trim_end_matches(|c| c == '\r' || c == '\n').to_string();
            let to_ua = detect_conversion_direction(&cleaned_text);
            let converted = convert_text(&cleaned_text, to_ua);
            
            clipboard.set_text(&converted)?;
            enigo.key(Key::Control, enigo::Direction::Press)?;
            enigo.key(Key::V, enigo::Direction::Click)?;
            enigo.key(Key::Control, enigo::Direction::Release)?;
            thread::sleep(time::Duration::from_millis(25));
        }

        spawn_clipboard_restore(saved_clipboard);
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