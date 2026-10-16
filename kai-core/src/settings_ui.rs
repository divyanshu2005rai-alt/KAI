//! Small native Windows settings window. It edits the same validated TOML
//! config as the command-line editor and stores keys in Credential Manager.

use crate::{
    config::Config,
    credentials,
    providers::{
        catalog::{CapabilityStatus, ModelInfo},
        CompletionRequest,
    },
    triggers::{TriggerDef, TriggersFile},
};
use anyhow::{bail, Result};
use std::os::windows::process::CommandExt;
use std::{mem, ptr};
use windows_sys::Win32::{
    Foundation::{HWND, LPARAM, LRESULT, WPARAM},
    Graphics::Gdi::{UpdateWindow, COLOR_BTNFACE},
    System::LibraryLoader::GetModuleHandleW,
    System::Threading::CREATE_NO_WINDOW,
    UI::Controls::{BST_CHECKED, BST_UNCHECKED},
    UI::Input::KeyboardAndMouse::EnableWindow,
    UI::WindowsAndMessaging::*,
};

const PROVIDER_ID: usize = 101;
const MODEL_ID: usize = 102;
const URL_ID: usize = 103;
const STRATEGY_ID: usize = 104;
const FALLBACK_ID: usize = 105;
const KEY_ID: usize = 106;
const SAVE_ID: usize = 107;
const REMOVE_KEY_ID: usize = 108;
const REFRESH_MODELS_ID: usize = 109;
const ROUTE_MODE_ID: usize = 110;
const MODEL_LIST_MESSAGE: u32 = WM_APP + 1;
const EDIT_TRIGGERS_ID: usize = 111;
const TEST_MODEL_ID: usize = 112;
const MODEL_TEST_MESSAGE: u32 = WM_APP + 2;
const START_ON_LOGIN_ID: usize = 113;

const TRIGGER_LIST_ID: usize = 201;
const TRIGGER_NAME_ID: usize = 202;
const TRIGGER_ALIASES_ID: usize = 203;
const TRIGGER_TOKENS_ID: usize = 204;
const TRIGGER_PROVIDER_ID: usize = 205;
const TRIGGER_MODEL_ID: usize = 206;
const TRIGGER_PROMPT_ID: usize = 207;
const FALLBACK_PROMPT_ID: usize = 208;
const TRIGGER_NEW_ID: usize = 209;
const TRIGGER_REMOVE_ID: usize = 210;
const TRIGGER_SAVE_ID: usize = 211;
const FALLBACK_SAVE_ID: usize = 212;
const TRIGGER_REFRESH_MODELS_ID: usize = 213;
const TRIGGER_MODEL_LIST_MESSAGE: u32 = WM_APP + 3;

struct SettingsWindow {
    config: Config,
    provider: HWND,
    model: HWND,
    base_url: HWND,
    strategy: HWND,
    fallback: HWND,
    key: HWND,
    status: HWND,
    refresh_models: HWND,
    routing_mode: HWND,
    start_on_login: HWND,
    start_on_login_enabled: bool,
}

struct ModelListUpdate {
    provider: String,
    result: Result<Vec<ModelInfo>, String>,
}

struct ModelTestUpdate {
    provider: String,
    model: String,
    result: Result<String, String>,
}

struct TriggerEditor {
    config: Config,
    triggers: TriggersFile,
    selected: Option<usize>,
    list: HWND,
    name: HWND,
    aliases: HWND,
    max_tokens: HWND,
    provider: HWND,
    model: HWND,
    refresh_models: HWND,
    route_hint: HWND,
    prompt: HWND,
    fallback_prompt: HWND,
    status: HWND,
}

pub fn run() -> Result<()> {
    let class_name = wide("KAISettingsWindow");
    let existing = unsafe { FindWindowW(class_name.as_ptr(), ptr::null()) };
    if !existing.is_null() {
        unsafe {
            ShowWindow(existing, SW_RESTORE);
            SetForegroundWindow(existing);
        }
        return Ok(());
    }

    let instance = unsafe { GetModuleHandleW(ptr::null()) };
    if instance.is_null() {
        bail!("could not locate KAI module handle");
    }
    let title = wide("KAI Settings");
    let class = WNDCLASSW {
        style: CS_HREDRAW | CS_VREDRAW,
        lpfnWndProc: Some(window_proc),
        hInstance: instance,
        hIcon: crate::tray::create_logo_icon(),
        hCursor: unsafe { LoadCursorW(ptr::null_mut(), IDC_ARROW) },
        hbrBackground: (COLOR_BTNFACE + 1) as _,
        lpszClassName: class_name.as_ptr(),
        ..unsafe { mem::zeroed() }
    };
    if unsafe { RegisterClassW(&class) } == 0 {
        bail!("could not register KAI settings window");
    }

    let mut state = Box::new(SettingsWindow {
        config: Config::load()?,
        provider: ptr::null_mut(),
        model: ptr::null_mut(),
        base_url: ptr::null_mut(),
        strategy: ptr::null_mut(),
        fallback: ptr::null_mut(),
        key: ptr::null_mut(),
        status: ptr::null_mut(),
        refresh_models: ptr::null_mut(),
        routing_mode: ptr::null_mut(),
        start_on_login: ptr::null_mut(),
        start_on_login_enabled: crate::startup::is_enabled()?,
    });
    let state_ptr = (&mut *state) as *mut SettingsWindow;
    let window = unsafe {
        CreateWindowExW(
            WS_EX_APPWINDOW,
            class_name.as_ptr(),
            title.as_ptr(),
            WS_OVERLAPPEDWINDOW | WS_VISIBLE,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            600,
            610,
            ptr::null_mut(),
            ptr::null_mut(),
            instance,
            state_ptr.cast(),
        )
    };
    if window.is_null() {
        bail!("could not create KAI settings window");
    }

    unsafe {
        let mut message: MSG = mem::zeroed();
        while GetMessageW(&mut message, ptr::null_mut(), 0, 0) > 0 {
            TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
    Ok(())
}

pub fn run_trigger_editor() -> Result<()> {
    let instance = unsafe { GetModuleHandleW(ptr::null()) };
    if instance.is_null() {
        bail!("could not locate KAI module handle");
    }
    let class_name = wide("KAITriggerEditorWindow");
    let title = wide("KAI Trigger Editor");
    let class = WNDCLASSW {
        style: CS_HREDRAW | CS_VREDRAW,
        lpfnWndProc: Some(trigger_window_proc),
        hInstance: instance,
        hIcon: crate::tray::create_logo_icon(),
        hCursor: unsafe { LoadCursorW(ptr::null_mut(), IDC_ARROW) },
        hbrBackground: (COLOR_BTNFACE + 1) as _,
        lpszClassName: class_name.as_ptr(),
        ..unsafe { mem::zeroed() }
    };
    if unsafe { RegisterClassW(&class) } == 0 {
        bail!("could not register KAI trigger editor window");
    }

    let mut state = Box::new(TriggerEditor {
        config: Config::load()?,
        triggers: TriggersFile::load()?,
        selected: None,
        list: ptr::null_mut(),
        name: ptr::null_mut(),
        aliases: ptr::null_mut(),
        max_tokens: ptr::null_mut(),
        provider: ptr::null_mut(),
        model: ptr::null_mut(),
        refresh_models: ptr::null_mut(),
        route_hint: ptr::null_mut(),
        prompt: ptr::null_mut(),
        fallback_prompt: ptr::null_mut(),
        status: ptr::null_mut(),
    });
    let state_ptr = (&mut *state) as *mut TriggerEditor;
    let window = unsafe {
        CreateWindowExW(
            WS_EX_APPWINDOW,
            class_name.as_ptr(),
            title.as_ptr(),
            WS_OVERLAPPEDWINDOW | WS_VISIBLE,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            800,
            800,
            ptr::null_mut(),
            ptr::null_mut(),
            instance,
            state_ptr.cast(),
        )
    };
    if window.is_null() {
        bail!("could not create KAI trigger editor window");
    }

    unsafe {
        let mut message: MSG = mem::zeroed();
        while GetMessageW(&mut message, ptr::null_mut(), 0, 0) > 0 {
            TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
    Ok(())
}

fn open_trigger_editor() -> Result<()> {
    let executable = std::env::current_exe()?;
    std::process::Command::new(executable)
        .arg("--edit-triggers")
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()?;
    Ok(())
}

unsafe extern "system" fn window_proc(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if message == WM_NCCREATE {
        let create = &*(lparam as *const CREATESTRUCTW);
        SetWindowLongPtrW(window, GWLP_USERDATA, create.lpCreateParams as isize);
    }
    let state_ptr = GetWindowLongPtrW(window, GWLP_USERDATA) as *mut SettingsWindow;

    match message {
        WM_CREATE => {
            if !state_ptr.is_null() {
                create_controls(window, &mut *state_ptr);
            }
            0
        }
        WM_COMMAND if !state_ptr.is_null() => {
            let id = (wparam & 0xffff) as usize;
            let notification = ((wparam >> 16) & 0xffff) as u32;
            let state = &mut *state_ptr;
            match (id, notification) {
                (PROVIDER_ID, CBN_SELCHANGE) => {
                    set_text(state.key, "");
                    load_provider_fields(state);
                }
                (SAVE_ID, BN_CLICKED) => save_settings(window, state),
                (REMOVE_KEY_ID, BN_CLICKED) => remove_key(window, state),
                (REFRESH_MODELS_ID, BN_CLICKED) => refresh_models(window, state),
                (TEST_MODEL_ID, BN_CLICKED) => test_model(window, state),
                (START_ON_LOGIN_ID, BN_CLICKED) => toggle_start_on_login(state),
                (EDIT_TRIGGERS_ID, BN_CLICKED) => {
                    if let Err(error) = open_trigger_editor() {
                        set_text(state.status, &format!("Could not open trigger editor: {error}"));
                    }
                }
                (ROUTE_MODE_ID, CBN_SELCHANGE) => set_text(
                    state.status,
                    match combo_selection(state.routing_mode) {
                        1 => "Manual mode prefers the selected provider/model; fallback may send prompts elsewhere.",
                        2 => "Per-trigger mode applies configured overrides; fallback may send prompts elsewhere.",
                        _ => "Automatic mode follows provider preference; fallback may switch routes.",
                    },
                ),
                _ => {}
            }
            0
        }
        MODEL_LIST_MESSAGE if !state_ptr.is_null() => {
            let update = Box::from_raw(lparam as *mut ModelListUpdate);
            let state = &mut *state_ptr;
            if selected_provider(state.provider) == update.provider {
                match update.result {
                    Ok(models) => {
                        let current_model = get_text(state.model);
                        SendMessageW(state.model, CB_RESETCONTENT, 0, 0);
                        for model in &models {
                            combo_add(state.model, &model.id);
                        }
                        set_text(state.model, &current_model);
                        let largest_context = models.iter().filter_map(|m| m.input_limit).max();
                        let largest_output = models.iter().filter_map(|m| m.output_limit).max();
                        let text_advertised = models
                            .iter()
                            .filter(|model| {
                                model.capabilities.text_chat == CapabilityStatus::Supported
                            })
                            .count();
                        let text_unverified = models.len().saturating_sub(text_advertised);
                        let image_advertised = models
                            .iter()
                            .filter(|model| {
                                model.capabilities.image_input == CapabilityStatus::Supported
                            })
                            .count();
                        let limits = match (largest_context, largest_output) {
                            (Some(input), Some(output)) => format!("{input}/{output}"),
                            (Some(input), None) => format!("{input}/unknown"),
                            (None, Some(output)) => format!("unknown/{output}"),
                            _ => "unknown/unknown".to_string(),
                        };
                        let image_unverified = models.len().saturating_sub(image_advertised);
                        set_text(
                            state.status,
                            &format!(
                                "{} models; max input/output {limits} tokens.\r\nText {text_advertised} advertised/{text_unverified} unknown; image {image_advertised} advertised/{image_unverified} unknown.",
                                models.len()
                            ),
                        );
                    }
                    Err(error) => {
                        set_text(state.status, &format!("Model list unavailable: {error}"))
                    }
                }
            }
            0
        }
        MODEL_TEST_MESSAGE if !state_ptr.is_null() => {
            let update = Box::from_raw(lparam as *mut ModelTestUpdate);
            let state = &mut *state_ptr;
            if selected_provider(state.provider) == update.provider
                && get_text(state.model).trim() == update.model
            {
                set_text(
                    state.status,
                    &match update.result {
                        Ok(message) => message,
                        Err(error) => format!("Model check failed: {error}"),
                    },
                );
            }
            0
        }
        WM_CLOSE => {
            DestroyWindow(window);
            0
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            0
        }
        _ => DefWindowProcW(window, message, wparam, lparam),
    }
}

unsafe extern "system" fn trigger_window_proc(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if message == WM_NCCREATE {
        let create = &*(lparam as *const CREATESTRUCTW);
        SetWindowLongPtrW(window, GWLP_USERDATA, create.lpCreateParams as isize);
    }
    let state_ptr = GetWindowLongPtrW(window, GWLP_USERDATA) as *mut TriggerEditor;

    match message {
        WM_CREATE => {
            if !state_ptr.is_null() {
                create_trigger_controls(window, &mut *state_ptr);
            }
            0
        }
        WM_COMMAND if !state_ptr.is_null() => {
            let id = (wparam & 0xffff) as usize;
            let notification = ((wparam >> 16) & 0xffff) as u32;
            let state = &mut *state_ptr;
            match (id, notification) {
                (TRIGGER_LIST_ID, CBN_SELCHANGE) => load_selected_trigger(state),
                (TRIGGER_PROVIDER_ID, CBN_SELCHANGE) => {
                    update_trigger_route_controls(state);
                }
                (TRIGGER_NEW_ID, BN_CLICKED) => begin_new_trigger(state),
                (TRIGGER_REMOVE_ID, BN_CLICKED) => remove_selected_trigger(window, state),
                (TRIGGER_REFRESH_MODELS_ID, BN_CLICKED) => refresh_trigger_models(window, state),
                (TRIGGER_SAVE_ID, BN_CLICKED) => save_trigger(state),
                (FALLBACK_SAVE_ID, BN_CLICKED) => save_fallback_prompt(state),
                _ => {}
            }
            0
        }
        TRIGGER_MODEL_LIST_MESSAGE if !state_ptr.is_null() => {
            let update = Box::from_raw(lparam as *mut ModelListUpdate);
            let state = &mut *state_ptr;
            if trigger_provider(combo_selection(state.provider)) == Some(update.provider.as_str()) {
                match update.result {
                    Ok(models) => {
                        let current_model = get_text(state.model);
                        SendMessageW(state.model, CB_RESETCONTENT, 0, 0);
                        for model in &models {
                            combo_add(state.model, &model.id);
                        }
                        set_text(state.model, &current_model);
                        let advertised_text = models
                            .iter()
                            .filter(|model| {
                                model.capabilities.text_chat == CapabilityStatus::Supported
                            })
                            .count();
                        set_text(
                            state.status,
                            &format!("Loaded {} IDs; {} advertise text chat. Test unverified choices in provider settings.", models.len(), advertised_text),
                        );
                    }
                    Err(error) => {
                        set_text(state.status, &format!("Could not load models: {error}"))
                    }
                }
            }
            0
        }
        WM_CLOSE => {
            DestroyWindow(window);
            0
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            0
        }
        _ => DefWindowProcW(window, message, wparam, lparam),
    }
}

unsafe fn create_trigger_controls(window: HWND, state: &mut TriggerEditor) {
    label(window, "Configured triggers", 20, 22, 145);
    state.list = combo(window, TRIGGER_LIST_ID, 170, 18, 310);
    create_control(
        window,
        "BUTTON",
        "New",
        WS_CHILD | WS_VISIBLE | WS_TABSTOP,
        495,
        18,
        90,
        28,
        TRIGGER_NEW_ID,
        0,
    );
    create_control(
        window,
        "BUTTON",
        "Remove",
        WS_CHILD | WS_VISIBLE | WS_TABSTOP,
        590,
        18,
        100,
        28,
        TRIGGER_REMOVE_ID,
        0,
    );

    label(window, "Command name", 20, 66, 145);
    state.name = edit(window, TRIGGER_NAME_ID, 170, 62, 520, false);
    label(window, "Aliases", 20, 106, 145);
    state.aliases = edit(window, TRIGGER_ALIASES_ID, 170, 102, 520, false);
    label(window, "Max response tokens", 20, 146, 145);
    state.max_tokens = edit(window, TRIGGER_TOKENS_ID, 170, 142, 160, false);

    label(window, "Provider", 20, 186, 145);
    state.provider = combo(window, TRIGGER_PROVIDER_ID, 170, 182, 260);
    for provider in ["Use default", "Groq", "Gemini", "OpenAI-compatible"] {
        combo_add(state.provider, provider);
    }
    label(window, "Model ID", 450, 186, 90);
    state.model = editable_combo(window, TRIGGER_MODEL_ID, 540, 182, 150);
    state.refresh_models = create_control(
        window,
        "BUTTON",
        "Load model IDs",
        WS_CHILD | WS_VISIBLE | WS_TABSTOP,
        540,
        210,
        150,
        26,
        TRIGGER_REFRESH_MODELS_ID,
        0,
    );
    state.route_hint = label(window, "", 170, 238, 520);
    EnableWindow(state.model, 0);
    EnableWindow(state.refresh_models, 0);

    label(window, "Instructions for this trigger", 20, 270, 300);
    state.prompt = multiline_edit(window, TRIGGER_PROMPT_ID, 20, 294, 670, 215);
    label(
        window,
        "Fallback instructions for unmatched commands",
        20,
        526,
        430,
    );
    state.fallback_prompt = multiline_edit(window, FALLBACK_PROMPT_ID, 20, 550, 670, 100);

    create_control(
        window,
        "BUTTON",
        "Save trigger",
        WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_DEFPUSHBUTTON as u32,
        170,
        668,
        130,
        32,
        TRIGGER_SAVE_ID,
        0,
    );
    create_control(
        window,
        "BUTTON",
        "Save fallback prompt",
        WS_CHILD | WS_VISIBLE | WS_TABSTOP,
        310,
        668,
        180,
        32,
        FALLBACK_SAVE_ID,
        0,
    );
    state.status = label(window, "", 20, 713, 680);
    update_trigger_route_controls(state);
    set_text(state.fallback_prompt, &state.triggers.fallback_prompt);
    refresh_trigger_list(state, None);
    if !state.triggers.triggers.is_empty() {
        combo_select(state.list, 0);
        load_selected_trigger(state);
    } else {
        begin_new_trigger(state);
    }
}

unsafe fn refresh_trigger_list(state: &TriggerEditor, select: Option<usize>) {
    SendMessageW(state.list, CB_RESETCONTENT, 0, 0);
    for trigger in &state.triggers.triggers {
        combo_add(state.list, &trigger.name);
    }
    if let Some(index) = select.filter(|index| *index < state.triggers.triggers.len()) {
        combo_select(state.list, index as i32);
    } else {
        combo_select(state.list, -1);
    }
}

unsafe fn load_selected_trigger(state: &mut TriggerEditor) {
    let index = combo_selection(state.list);
    if index < 0 {
        begin_new_trigger(state);
        return;
    }
    let index = index as usize;
    let Some(trigger) = state.triggers.triggers.get(index) else {
        return;
    };
    state.selected = Some(index);
    set_text(state.name, &trigger.name);
    set_text(state.aliases, &trigger.aliases.join(", "));
    set_text(
        state.max_tokens,
        &trigger
            .max_tokens
            .map(|value| value.to_string())
            .unwrap_or_default(),
    );
    combo_select(
        state.provider,
        trigger
            .provider
            .as_deref()
            .map(trigger_provider_index)
            .unwrap_or(0),
    );
    set_text(state.model, trigger.model.as_deref().unwrap_or(""));
    update_trigger_route_controls(state);
    set_text(state.prompt, &trigger.prompt);
    set_text(state.status, "Edit the fields, then save the trigger.");
}

unsafe fn begin_new_trigger(state: &mut TriggerEditor) {
    state.selected = None;
    combo_select(state.list, -1);
    set_text(state.name, "");
    set_text(state.aliases, "");
    set_text(state.max_tokens, "");
    combo_select(state.provider, 0);
    set_text(state.model, "");
    update_trigger_route_controls(state);
    set_text(state.prompt, "");
    set_text(
        state.status,
        "Enter a command name and instructions, then save it.",
    );
}

unsafe fn update_trigger_route_controls(state: &TriggerEditor) {
    let routing_enabled = trigger_routing_enabled(&state.config.routing.mode);
    let provider_selected = combo_selection(state.provider) != 0;
    EnableWindow(state.provider, routing_enabled as i32);
    EnableWindow(state.model, (routing_enabled && provider_selected) as i32);
    EnableWindow(
        state.refresh_models,
        (routing_enabled && provider_selected) as i32,
    );
    set_text(
        state.route_hint,
        if routing_enabled {
            "Use default or choose a model for this trigger."
        } else {
            "Enable per-trigger routing in Settings first."
        },
    );
}

fn trigger_routing_enabled(mode: &str) -> bool {
    mode.eq_ignore_ascii_case("trigger-based")
}

unsafe fn save_trigger(state: &mut TriggerEditor) {
    let name = get_text(state.name).trim().to_string();
    let aliases = get_text(state.aliases)
        .split(',')
        .map(str::trim)
        .filter(|alias| !alias.is_empty())
        .map(str::to_string)
        .collect::<Vec<_>>();
    let max_tokens = match get_text(state.max_tokens).trim() {
        "" => None,
        value => match value.parse::<u32>() {
            Ok(tokens) => Some(tokens),
            Err(_) => {
                set_text(state.status, "Max response tokens must be a whole number.");
                return;
            }
        },
    };
    let provider = trigger_provider(combo_selection(state.provider));
    let model = get_text(state.model).trim().to_string();
    let trigger = TriggerDef {
        name,
        aliases,
        prompt: get_text(state.prompt).trim().to_string(),
        max_tokens,
        provider: provider.map(str::to_string),
        model: if model.is_empty() { None } else { Some(model) },
    };

    let mut candidate = state.triggers.clone();
    let selected = if let Some(index) = state.selected {
        if index >= candidate.triggers.len() {
            set_text(
                state.status,
                "The selected trigger no longer exists. Reload the editor.",
            );
            return;
        }
        candidate.triggers[index] = trigger;
        index
    } else {
        candidate.triggers.push(trigger);
        candidate.triggers.len() - 1
    };
    if let Err(error) = candidate.validate().and_then(|()| candidate.save()) {
        set_text(state.status, &format!("Trigger was not saved: {error}"));
        return;
    }
    state.triggers = candidate;
    state.selected = Some(selected);
    refresh_trigger_list(state, Some(selected));
    load_selected_trigger(state);
    set_text(state.status, "Trigger saved to triggers.yml.");
}

unsafe fn save_fallback_prompt(state: &mut TriggerEditor) {
    let mut candidate = state.triggers.clone();
    candidate.fallback_prompt = get_text(state.fallback_prompt).trim().to_string();
    if let Err(error) = candidate.validate().and_then(|()| candidate.save()) {
        set_text(
            state.status,
            &format!("Fallback prompt was not saved: {error}"),
        );
        return;
    }
    state.triggers = candidate;
    set_text(state.status, "Fallback prompt saved to triggers.yml.");
}

unsafe fn remove_selected_trigger(window: HWND, state: &mut TriggerEditor) {
    let Some(index) = state.selected else {
        set_text(state.status, "Select a trigger before removing it.");
        return;
    };
    let message = wide("Remove the selected trigger from triggers.yml?");
    let title = wide("Remove KAI trigger");
    if MessageBoxW(
        window,
        message.as_ptr(),
        title.as_ptr(),
        MB_YESNO | MB_ICONWARNING,
    ) != IDYES
    {
        return;
    }
    let mut candidate = state.triggers.clone();
    candidate.triggers.remove(index);
    if let Err(error) = candidate.validate().and_then(|()| candidate.save()) {
        set_text(state.status, &format!("Trigger was not removed: {error}"));
        return;
    }
    state.triggers = candidate;
    refresh_trigger_list(state, None);
    begin_new_trigger(state);
    set_text(state.status, "Trigger removed from triggers.yml.");
}

unsafe fn create_controls(window: HWND, state: &mut SettingsWindow) {
    label(window, "Routing mode", 24, 24, 150);
    state.routing_mode = combo(window, ROUTE_MODE_ID, 175, 20, 315);
    for mode in ["Automatic", "Manual provider", "Per-trigger routing"] {
        combo_add(state.routing_mode, mode);
    }

    label(window, "Provider / fallback", 24, 65, 150);
    state.provider = combo(window, PROVIDER_ID, 175, 61, 315);
    for provider in ["Automatic", "Groq", "Gemini", "OpenAI-compatible"] {
        combo_add(state.provider, provider);
    }

    label(window, "Model", 24, 108, 150);
    state.model = editable_combo(window, MODEL_ID, 175, 104, 180);
    state.refresh_models = create_control(
        window,
        "BUTTON",
        "Refresh list",
        WS_CHILD | WS_VISIBLE | WS_TABSTOP,
        365,
        104,
        85,
        26,
        REFRESH_MODELS_ID,
        0,
    );
    create_control(
        window,
        "BUTTON",
        "Test model",
        WS_CHILD | WS_VISIBLE | WS_TABSTOP,
        455,
        104,
        80,
        26,
        TEST_MODEL_ID,
        0,
    );
    label(window, "Load models or enter a model ID.", 175, 132, 315);

    label(window, "Local/API base URL", 24, 170, 150);
    state.base_url = edit(window, URL_ID, 175, 166, 315, false);

    label(window, "Automatic preference", 24, 210, 150);
    state.strategy = combo(window, STRATEGY_ID, 175, 206, 315);
    for strategy in ["Automatic with fallback", "Cloud", "Local-first"] {
        combo_add(state.strategy, strategy);
    }

    state.fallback = create_control(
        window,
        "BUTTON",
        "Fallback can send prompts to another provider",
        WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_AUTOCHECKBOX as u32,
        175,
        251,
        390,
        26,
        FALLBACK_ID,
        0,
    );

    label(window, "API key", 24, 291, 150);
    state.key = edit(window, KEY_ID, 175, 287, 315, true);
    label(
        window,
        "Blank keeps the saved key in Credential Manager.",
        24,
        315,
        470,
    );

    create_control(
        window,
        "BUTTON",
        "Save settings",
        WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_DEFPUSHBUTTON as u32,
        175,
        355,
        130,
        32,
        SAVE_ID,
        0,
    );
    create_control(
        window,
        "BUTTON",
        "Remove saved key",
        WS_CHILD | WS_VISIBLE | WS_TABSTOP,
        315,
        355,
        175,
        32,
        REMOVE_KEY_ID,
        0,
    );
    state.status = create_control(
        window,
        "STATIC",
        "",
        WS_CHILD | WS_VISIBLE,
        24,
        398,
        470,
        34,
        0,
        0,
    );
    create_control(
        window,
        "BUTTON",
        "Edit triggers…",
        WS_CHILD | WS_VISIBLE | WS_TABSTOP,
        175,
        425,
        175,
        32,
        EDIT_TRIGGERS_ID,
        0,
    );
    state.start_on_login = create_control(
        window,
        "BUTTON",
        "Start KAI when I sign in (runs in background)",
        WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_AUTOCHECKBOX as u32,
        175,
        468,
        390,
        26,
        START_ON_LOGIN_ID,
        0,
    );

    combo_select(
        state.routing_mode,
        route_mode_index(
            &state.config.routing.mode,
            state
                .config
                .providers
                .active_provider
                .as_deref()
                .unwrap_or("auto"),
        ),
    );

    combo_select(
        state.provider,
        provider_index(
            state
                .config
                .providers
                .active_provider
                .as_deref()
                .unwrap_or("auto"),
        ),
    );
    combo_select(
        state.strategy,
        strategy_index(&state.config.routing.strategy),
    );
    check(state.fallback, state.config.routing.fallback);
    check(state.start_on_login, state.start_on_login_enabled);
    load_provider_fields(state);
}

unsafe fn toggle_start_on_login(state: &mut SettingsWindow) {
    let enabled = SendMessageW(state.start_on_login, BM_GETCHECK, 0, 0) == BST_CHECKED as isize;
    match crate::startup::set_enabled(enabled) {
        Ok(()) => {
            state.start_on_login_enabled = enabled;
            set_text(
                state.status,
                if enabled {
                    "KAI will start in the background the next time you sign in."
                } else {
                    "KAI will no longer start automatically when you sign in."
                },
            );
        }
        Err(error) => {
            check(state.start_on_login, state.start_on_login_enabled);
            set_text(
                state.status,
                &format!("Could not update sign-in startup: {error}"),
            );
        }
    }
}

unsafe fn load_provider_fields(state: &mut SettingsWindow) {
    let provider = selected_provider(state.provider);
    let (model, url, key_target) = match provider {
        "groq" => (
            state
                .config
                .providers
                .groq
                .as_ref()
                .and_then(|p| p.model.as_deref())
                .unwrap_or("openai/gpt-oss-120b"),
            "",
            "KAI/GROQ_API_KEY",
        ),
        "gemini" => (
            state
                .config
                .providers
                .gemini
                .as_ref()
                .and_then(|p| p.default_model.as_deref())
                .unwrap_or("gemini-2.5-flash"),
            "",
            "KAI/GEMINI_API_KEY",
        ),
        "openai-compatible" => (
            state
                .config
                .providers
                .openai_compatible
                .as_ref()
                .map(|p| p.default_model.as_str())
                .unwrap_or("llama3.2"),
            state
                .config
                .providers
                .openai_compatible
                .as_ref()
                .map(|p| p.base_url.as_str())
                .unwrap_or("http://127.0.0.1:11434/v1"),
            "KAI/OPENAI_COMPATIBLE_API_KEY",
        ),
        _ => ("", "", ""),
    };
    set_text(state.model, model);
    set_text(state.base_url, url);
    EnableWindow(state.base_url, (provider == "openai-compatible") as i32);
    EnableWindow(state.key, (!key_target.is_empty()) as i32);
    EnableWindow(state.model, (!key_target.is_empty()) as i32);
    EnableWindow(state.refresh_models, (!key_target.is_empty()) as i32);
    if !key_target.is_empty() {
        let saved = credentials::read_secret(key_target).is_some();
        set_text(
            state.status,
            if saved {
                "A key is saved for this provider."
            } else {
                "No key saved for this provider."
            },
        );
    } else {
        set_text(
            state.status,
            "Automatic routing uses the configured providers and fallback policy.",
        );
    }
}

unsafe fn save_settings(window: HWND, state: &mut SettingsWindow) {
    let provider = selected_provider(state.provider).to_string();
    let model = get_text(state.model);
    let base_url = get_text(state.base_url);
    let strategy = match combo_selection(state.strategy) {
        1 => "cloud",
        2 => "local",
        _ => "auto",
    };
    state.config.routing.mode = match combo_selection(state.routing_mode) {
        1 => "manual",
        2 => "trigger-based",
        _ => "automatic",
    }
    .to_string();
    state.config.providers.active_provider = Some(if state.config.routing.mode == "automatic" {
        "auto".to_string()
    } else {
        provider.clone()
    });
    state.config.routing.strategy = strategy.to_string();
    state.config.routing.fallback =
        SendMessageW(state.fallback, BM_GETCHECK, 0, 0) == BST_CHECKED as isize;

    match provider.as_str() {
        "groq" => {
            state
                .config
                .providers
                .groq
                .get_or_insert_with(|| crate::config::GroqConfig { model: None })
                .model = Some(model);
        }
        "gemini" => {
            state
                .config
                .providers
                .gemini
                .get_or_insert_with(|| crate::config::GeminiConfig {
                    default_model: None,
                    legacy_api_key: None,
                })
                .default_model = Some(model);
        }
        "openai-compatible" => {
            state
                .config
                .providers
                .openai_compatible
                .get_or_insert_with(|| crate::config::OpenAiCompatibleConfig {
                    base_url: String::new(),
                    default_model: String::new(),
                    env_key: None,
                    legacy_api_key: None,
                })
                .base_url = base_url;
            if let Some(local) = state.config.providers.openai_compatible.as_mut() {
                local.default_model = model;
            }
        }
        _ => {}
    }

    if let Err(error) = state.config.validate() {
        set_text(state.status, &format!("Settings are invalid: {error}"));
        return;
    }

    let key = get_text(state.key);
    let key_target = match provider.as_str() {
        "groq" => Some("KAI/GROQ_API_KEY"),
        "gemini" => Some("KAI/GEMINI_API_KEY"),
        "openai-compatible" => Some("KAI/OPENAI_COMPATIBLE_API_KEY"),
        _ => None,
    };
    if !key.trim().is_empty() {
        if let Some(target) = key_target {
            if let Err(error) = credentials::write_secret(target, key.trim()) {
                set_text(state.status, &format!("Could not save API key: {error}"));
                return;
            }
        }
        set_text(state.key, "");
    }

    match state.config.save() {
        Ok(()) => set_text(
            state.status,
            "Settings saved. Restart KAI to apply provider changes.",
        ),
        Err(error) => set_text(state.status, &format!("Could not save settings: {error}")),
    }
    let _ = window;
}

unsafe fn remove_key(_window: HWND, state: &mut SettingsWindow) {
    let target = match selected_provider(state.provider) {
        "groq" => Some("KAI/GROQ_API_KEY"),
        "gemini" => Some("KAI/GEMINI_API_KEY"),
        "openai-compatible" => Some("KAI/OPENAI_COMPATIBLE_API_KEY"),
        _ => None,
    };
    if let Some(target) = target {
        match credentials::delete_secret(target) {
            Ok(()) => set_text(state.status, "Saved API key removed."),
            Err(error) => set_text(state.status, &format!("Could not remove API key: {error}")),
        }
    }
}

unsafe fn refresh_models(window: HWND, state: &SettingsWindow) {
    let provider = selected_provider(state.provider).to_string();
    if provider == "auto" {
        set_text(
            state.status,
            "Choose a specific provider to list its models.",
        );
        return;
    }

    let mut config = state.config.clone();
    if provider == "openai-compatible" {
        let base_url = get_text(state.base_url);
        let model = get_text(state.model);
        let local = config.providers.openai_compatible.get_or_insert_with(|| {
            crate::config::OpenAiCompatibleConfig {
                base_url: String::new(),
                default_model: String::new(),
                env_key: None,
                legacy_api_key: None,
            }
        });
        if !base_url.trim().is_empty() {
            local.base_url = base_url;
        }
        if !model.trim().is_empty() {
            local.default_model = model;
        }
    }

    set_text(state.status, "Loading provider models…");
    let _ = UpdateWindow(window);
    let window_bits = window as usize;
    std::thread::spawn(move || {
        let result = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|error| error.to_string())
            .and_then(|runtime| {
                runtime
                    .block_on(crate::providers::catalog::list(&config, &provider))
                    .map_err(|error| error.to_string())
            });
        let update = Box::new(ModelListUpdate { provider, result });
        let update_ptr = Box::into_raw(update);
        let posted = PostMessageW(
            window_bits as HWND,
            MODEL_LIST_MESSAGE,
            0,
            update_ptr as LPARAM,
        );
        if posted == 0 {
            drop(Box::from_raw(update_ptr));
        }
    });
}

unsafe fn refresh_trigger_models(window: HWND, state: &TriggerEditor) {
    let Some(provider) = trigger_provider(combo_selection(state.provider)) else {
        set_text(state.status, "Choose a provider before loading model IDs.");
        return;
    };
    let provider = provider.to_string();
    let config = state.config.clone();
    set_text(state.status, "Loading provider model IDs…");
    let _ = UpdateWindow(window);
    let window_bits = window as usize;
    std::thread::spawn(move || {
        let result = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|error| error.to_string())
            .and_then(|runtime| {
                runtime
                    .block_on(crate::providers::catalog::list(&config, &provider))
                    .map_err(|error| error.to_string())
            });
        let update = Box::new(ModelListUpdate { provider, result });
        let update_ptr = Box::into_raw(update);
        if PostMessageW(
            window_bits as HWND,
            TRIGGER_MODEL_LIST_MESSAGE,
            0,
            update_ptr as LPARAM,
        ) == 0
        {
            drop(Box::from_raw(update_ptr));
        }
    });
}

unsafe fn test_model(window: HWND, state: &SettingsWindow) {
    let provider = selected_provider(state.provider).to_string();
    let model = get_text(state.model).trim().to_string();
    if provider == "auto" {
        set_text(
            state.status,
            "Choose a specific provider before testing a model.",
        );
        return;
    }
    if model.is_empty() {
        set_text(state.status, "Choose or enter a model ID before testing.");
        return;
    }

    let mut config = state.config.clone();
    if provider == "openai-compatible" {
        let local = config.providers.openai_compatible.get_or_insert_with(|| {
            crate::config::OpenAiCompatibleConfig {
                base_url: String::new(),
                default_model: String::new(),
                env_key: None,
                legacy_api_key: None,
            }
        });
        local.base_url = get_text(state.base_url).trim().to_string();
    }

    set_text(
        state.status,
        "Testing with a small request; provider usage may be billed…",
    );
    let _ = UpdateWindow(window);
    let window_bits = window as usize;
    std::thread::spawn(move || {
        let result = (|| {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|error| error.to_string())?;
            let provider_impl = crate::providers::factory::build_exact(&config, &provider, &model)
                .map_err(|error| error.to_string())?;
            runtime
                .block_on(async {
                    tokio::time::timeout(
                        std::time::Duration::from_secs(20),
                        provider_impl.complete(CompletionRequest {
                            system_prompt: "KAI model compatibility check.".to_string(),
                            user_prompt: "Reply exactly: KAI_OK".to_string(),
                            history: Vec::new(),
                            max_tokens: Some(8),
                            stream: false,
                        }),
                    )
                    .await
                })
                .map_err(|_| "request timed out after 20 seconds".to_string())?
                .map(|response| {
                    if response.model == model {
                        format!("Model responded successfully: {}.", response.model)
                    } else {
                        format!(
                            "Request succeeded via {}; provider substituted selected model {}.",
                            response.model, model
                        )
                    }
                })
                .map_err(|error| error.to_string())
        })();
        let update = Box::new(ModelTestUpdate {
            provider,
            model,
            result,
        });
        let update_ptr = Box::into_raw(update);
        if PostMessageW(
            window_bits as HWND,
            MODEL_TEST_MESSAGE,
            0,
            update_ptr as LPARAM,
        ) == 0
        {
            drop(Box::from_raw(update_ptr));
        }
    });
}

unsafe fn create_control(
    parent: HWND,
    class: &str,
    caption: &str,
    style: WINDOW_STYLE,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    id: usize,
    ex_style: WINDOW_EX_STYLE,
) -> HWND {
    let class = wide(class);
    let caption = wide(caption);
    CreateWindowExW(
        ex_style,
        class.as_ptr(),
        caption.as_ptr(),
        style,
        x,
        y,
        width,
        height,
        parent,
        id as isize as _,
        GetModuleHandleW(ptr::null()),
        ptr::null(),
    )
}

unsafe fn label(parent: HWND, text: &str, x: i32, y: i32, width: i32) -> HWND {
    create_control(
        parent,
        "STATIC",
        text,
        WS_CHILD | WS_VISIBLE,
        x,
        y,
        width,
        24,
        0,
        0,
    )
}

unsafe fn edit(parent: HWND, id: usize, x: i32, y: i32, width: i32, password: bool) -> HWND {
    let mut style = WS_CHILD | WS_VISIBLE | WS_TABSTOP | ES_AUTOHSCROLL as u32;
    if password {
        style |= ES_PASSWORD as u32;
    }
    create_control(
        parent,
        "EDIT",
        "",
        style,
        x,
        y,
        width,
        26,
        id,
        WS_EX_CLIENTEDGE,
    )
}

unsafe fn multiline_edit(parent: HWND, id: usize, x: i32, y: i32, width: i32, height: i32) -> HWND {
    create_control(
        parent,
        "EDIT",
        "",
        WS_CHILD
            | WS_VISIBLE
            | WS_TABSTOP
            | ES_MULTILINE as u32
            | ES_AUTOVSCROLL as u32
            | WS_VSCROLL,
        x,
        y,
        width,
        height,
        id,
        WS_EX_CLIENTEDGE,
    )
}

unsafe fn combo(parent: HWND, id: usize, x: i32, y: i32, width: i32) -> HWND {
    create_control(
        parent,
        "COMBOBOX",
        "",
        WS_CHILD | WS_VISIBLE | WS_TABSTOP | CBS_DROPDOWNLIST as u32 | WS_VSCROLL,
        x,
        y,
        width,
        160,
        id,
        WS_EX_CLIENTEDGE,
    )
}

unsafe fn editable_combo(parent: HWND, id: usize, x: i32, y: i32, width: i32) -> HWND {
    create_control(
        parent,
        "COMBOBOX",
        "",
        WS_CHILD | WS_VISIBLE | WS_TABSTOP | CBS_DROPDOWN as u32 | WS_VSCROLL,
        x,
        y,
        width,
        180,
        id,
        WS_EX_CLIENTEDGE,
    )
}

unsafe fn combo_add(combo: HWND, text: &str) {
    let text = wide(text);
    SendMessageW(combo, CB_ADDSTRING, 0, text.as_ptr() as LPARAM);
}

unsafe fn combo_select(combo: HWND, index: i32) {
    SendMessageW(combo, CB_SETCURSEL, index as WPARAM, 0);
}

unsafe fn combo_selection(combo: HWND) -> i32 {
    SendMessageW(combo, CB_GETCURSEL, 0, 0) as i32
}

unsafe fn selected_provider(combo: HWND) -> &'static str {
    match combo_selection(combo) {
        1 => "groq",
        2 => "gemini",
        3 => "openai-compatible",
        _ => "auto",
    }
}

fn trigger_provider(index: i32) -> Option<&'static str> {
    match index {
        1 => Some("groq"),
        2 => Some("gemini"),
        3 => Some("openai-compatible"),
        _ => None,
    }
}

fn trigger_provider_index(provider: &str) -> i32 {
    match provider.to_ascii_lowercase().as_str() {
        "groq" => 1,
        "gemini" => 2,
        "openai-compatible" => 3,
        _ => 0,
    }
}

fn provider_index(provider: &str) -> i32 {
    match provider.to_ascii_lowercase().as_str() {
        "groq" => 1,
        "gemini" => 2,
        "openai-compatible" | "local" => 3,
        _ => 0,
    }
}

fn strategy_index(strategy: &str) -> i32 {
    match strategy.to_ascii_lowercase().as_str() {
        "cloud" => 1,
        "local" => 2,
        _ => 0,
    }
}

fn route_mode_index(mode: &str, active_provider: &str) -> i32 {
    match mode.to_ascii_lowercase().as_str() {
        "trigger-based" => 2,
        "manual" => 1,
        _ if active_provider != "auto" && active_provider != "cloud" => 1,
        _ => 0,
    }
}

unsafe fn check(button: HWND, checked: bool) {
    let value = if checked { BST_CHECKED } else { BST_UNCHECKED };
    SendMessageW(button, BM_SETCHECK, value as WPARAM, 0);
}

unsafe fn set_text(window: HWND, text: &str) {
    let text = wide(text);
    SetWindowTextW(window, text.as_ptr());
}

unsafe fn get_text(window: HWND) -> String {
    let length = GetWindowTextLengthW(window).max(0) as usize;
    let mut buffer = vec![0u16; length + 1];
    let actual = GetWindowTextW(window, buffer.as_mut_ptr(), buffer.len() as i32).max(0) as usize;
    String::from_utf16_lossy(&buffer[..actual])
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(test)]
mod tests {
    use super::trigger_routing_enabled;

    #[test]
    fn trigger_overrides_only_apply_in_per_trigger_mode() {
        assert!(trigger_routing_enabled("trigger-based"));
        assert!(trigger_routing_enabled("TRIGGER-BASED"));
        assert!(!trigger_routing_enabled("automatic"));
        assert!(!trigger_routing_enabled("manual"));
    }
}
