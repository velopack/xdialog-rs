#![allow(dead_code)]

use std::collections::HashMap;
use std::ptr::null_mut;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};

use widestring::U16CString;
use windows::core::{HRESULT, PCWSTR};
use windows::Win32::Foundation::{BOOL, FALSE, HMODULE, HWND, LPARAM, S_OK, TRUE, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Controls::{
    TaskDialogIndirect, TASKDIALOGCONFIG, TASKDIALOGCONFIG_0, TASKDIALOGCONFIG_1, TASKDIALOG_BUTTON, TASKDIALOG_COMMON_BUTTON_FLAGS,
    TASKDIALOG_FLAGS, TASKDIALOG_NOTIFICATIONS, TDE_CONTENT, TDE_EXPANDED_INFORMATION, TDE_FOOTER, TDE_MAIN_INSTRUCTION,
    TDF_CALLBACK_TIMER, TDF_SHOW_PROGRESS_BAR, TDF_SIZE_TO_CONTENT, TDM_SET_BUTTON_ELEVATION_REQUIRED_STATE, TDM_SET_ELEMENT_TEXT,
    TDM_SET_MARQUEE_PROGRESS_BAR, TDM_SET_PROGRESS_BAR_MARQUEE, TDM_SET_PROGRESS_BAR_POS, TDN_CREATED, TDN_DESTROYED,
    TDN_HYPERLINK_CLICKED, TDN_TIMER, TD_ERROR_ICON, TD_INFORMATION_ICON, TD_WARNING_ICON,
};
use windows::Win32::UI::WindowsAndMessaging::{EndDialog, SendMessageW, HICON};

use crate::{backends::DialogManager, insert_result, XDialogError, XDialogIcon, XDialogOptions, XDialogResult};

#[derive(Debug, PartialEq)]
enum DialogRequest {
    None,
    Close,
    SetProgress(f32),
    SetIndeterminate,
    SetText(String),
}

pub struct TaskDialogManager {
    open_dialogs: Arc<Mutex<HashMap<usize, (Sender<DialogRequest>, Receiver<DialogRequest>)>>>,
}

impl TaskDialogManager {
    pub fn new() -> Self {
        TaskDialogManager { open_dialogs: Arc::new(Mutex::new(HashMap::new())) }
    }
}

impl DialogManager for TaskDialogManager {
    fn show(&mut self, id: usize, data: XDialogOptions, has_progress: bool) -> Result<(), XDialogError> {
        let open_dialogs = self.open_dialogs.clone();
        // Insert a new dialog
        {
            let mut dialogs = self.open_dialogs.lock().unwrap();
            let (sender, receiver) = channel();
            dialogs.insert(id, (sender, receiver));
        }

        std::thread::spawn(move || {
            let mut config = TaskDialogConfig::new(open_dialogs.clone());
            config.window_title = data.title;
            config.main_instruction = data.main_instruction;
            config.content = data.message;
            config.x_dialog_id = id;
            let mut default_button: Option<i32> = None;
            for (idx, text) in data.buttons.iter().enumerate().rev() {
                if default_button.is_none() {
                    default_button = Some(idx as i32);
                }

                let button = TaskDialogButton { text: text.clone(), id: idx as i32 };
                config.buttons.push(button);
            }
            config.default_button = default_button.unwrap_or(0);
            config.main_icon = convert_icon(data.icon);
            config.progress = if has_progress { ProgressState::Pos(0f32) } else { ProgressState::None };
            config.flags = TDF_SIZE_TO_CONTENT | TDF_CALLBACK_TIMER;
            if has_progress {
                config.flags |= TDF_SHOW_PROGRESS_BAR;
            }
            config.callback = Some(|hwnd, msg, _w_param, _l_param, ref_data| {
                if msg == TDN_TIMER {
                    let config = unsafe { &mut *ref_data };
                    let open_dialogs = config.open_dialogs.clone();
                    let mut open_dialogs = open_dialogs.lock().unwrap();

                    if let Some(state) = open_dialogs.get_mut(&config.x_dialog_id) {
                        let mut desired_state = ProgressState::None;
                        loop {
                            // read all messages until there are no more queued
                            let message = state.1.try_recv().unwrap_or(DialogRequest::None);
                            match message {
                                DialogRequest::None => break,
                                DialogRequest::Close => unsafe {
                                    let _ = EndDialog(hwnd, -1);
                                },
                                DialogRequest::SetProgress(val) => desired_state = ProgressState::Pos(val),
                                DialogRequest::SetIndeterminate => desired_state = ProgressState::Indeterminate,
                                DialogRequest::SetText(text) => config.set_content(&text),
                            }
                        }

                        if desired_state != ProgressState::None {
                            if let ProgressState::Pos(progress) = desired_state {
                                if config.progress == ProgressState::Indeterminate {
                                    config.set_progress_bar_marquee_on_off(false);
                                    config.set_progress_bar_marquee_progress(false);
                                }

                                config.set_progress_bar_pos((progress * 100f32) as usize);
                                config.progress = ProgressState::Pos(progress);
                            } else if ProgressState::Indeterminate == desired_state {
                                config.set_progress_bar_marquee_on_off(true);
                                config.set_progress_bar_marquee_progress(true);
                                config.progress = ProgressState::Indeterminate;
                            }
                        }
                    }
                }
                S_OK
            });

            let result = unsafe { execute_task_dialog(&mut config) };

            // Remove dialog
            {
                let mut dialogs = open_dialogs.lock().unwrap();
                dialogs.remove(&id);
            }

            let xresult = match result {
                Ok(result) => {
                    if result.button_id < 0 {
                        XDialogResult::WindowClosed
                    } else {
                        XDialogResult::ButtonPressed(result.button_id as usize)
                    }
                }
                Err(_) => XDialogResult::WindowClosed,
            };

            insert_result(id, xresult);
        });

        Ok(())
    }

    fn close(&mut self, id: usize) {
        if let Some(obj) = self.open_dialogs.lock().unwrap().get_mut(&id) {
            let _ = obj.0.send(DialogRequest::Close);
        }
    }

    fn close_all(&mut self) {
        let mut dialogs = self.open_dialogs.lock().unwrap();
        for (_id, obj) in dialogs.iter_mut() {
            let _ = obj.0.send(DialogRequest::Close);
        }
    }

    fn set_progress_value(&mut self, id: usize, progress: f32) {
        if let Some(obj) = self.open_dialogs.lock().unwrap().get_mut(&id) {
            let _ = obj.0.send(DialogRequest::SetProgress(progress));
        }
    }

    fn set_progress_text(&mut self, id: usize, text: &str) {
        if let Some(obj) = self.open_dialogs.lock().unwrap().get_mut(&id) {
            let _ = obj.0.send(DialogRequest::SetText(text.to_string()));
        }
    }

    fn set_progress_indeterminate(&mut self, id: usize) {
        if let Some(obj) = self.open_dialogs.lock().unwrap().get_mut(&id) {
            let _ = obj.0.send(DialogRequest::SetIndeterminate);
        }
    }
}

type TaskDialogHyperlinkCallback = Option<fn(context: &str) -> ()>;

type TaskDialogWndProcCallback =
    Option<fn(hwnd: HWND, msg: TASKDIALOG_NOTIFICATIONS, w_param: WPARAM, l_param: LPARAM, ref_data: *mut TaskDialogConfig) -> HRESULT>;

fn convert_icon(icon: XDialogIcon) -> TASKDIALOGCONFIG_0 {
    match icon {
        XDialogIcon::None => TASKDIALOGCONFIG_0 { hMainIcon: HICON(null_mut()) },
        XDialogIcon::Error => TASKDIALOGCONFIG_0 { pszMainIcon: TD_ERROR_ICON },
        XDialogIcon::Warning => TASKDIALOGCONFIG_0 { pszMainIcon: TD_WARNING_ICON },
        XDialogIcon::Information => TASKDIALOGCONFIG_0 { pszMainIcon: TD_INFORMATION_ICON },
    }
}

#[derive(Debug, PartialEq)]
enum ProgressState {
    None,
    Indeterminate,
    Pos(f32),
}

struct TaskDialogConfig {
    pub parent: HWND,
    pub instance: HMODULE,
    pub flags: TASKDIALOG_FLAGS,
    pub common_buttons: TASKDIALOG_COMMON_BUTTON_FLAGS,
    pub window_title: String,
    pub main_instruction: String,
    pub content: String,
    pub verification_text: String,
    pub expanded_information: String,
    pub expanded_control_text: String,
    pub collapsed_control_text: String,
    pub footer: String,
    pub buttons: Vec<TaskDialogButton>,
    pub default_button: i32,
    pub radio_buttons: Vec<TaskDialogButton>,
    pub default_radio_buttons: i32,
    pub main_icon: TASKDIALOGCONFIG_0,
    pub footer_icon: TASKDIALOGCONFIG_1,
    /** When created dialog, the value set to HWND. */
    pub dialog_hwnd: HWND,
    /** When close the dialog, the value set to true, default is false. */
    pub is_destroyed: bool,
    pub hyperlink_callback: TaskDialogHyperlinkCallback,
    pub callback: TaskDialogWndProcCallback,
    pub cx_width: u32,
    pub progress: ProgressState,
    pub x_dialog_id: usize,
    pub open_dialogs: Arc<Mutex<HashMap<usize, (Sender<DialogRequest>, Receiver<DialogRequest>)>>>,
}

impl TaskDialogConfig {
    fn new(open_dialogs: Arc<Mutex<HashMap<usize, (Sender<DialogRequest>, Receiver<DialogRequest>)>>>) -> Self {
        TaskDialogConfig {
            parent: HWND(null_mut()),
            instance: HMODULE(null_mut()),
            flags: TASKDIALOG_FLAGS(0),
            common_buttons: TASKDIALOG_COMMON_BUTTON_FLAGS(0),
            window_title: "".to_string(),
            main_instruction: "".to_string(),
            content: "".to_string(),
            verification_text: "".to_string(),
            expanded_information: "".to_string(),
            expanded_control_text: "".to_string(),
            collapsed_control_text: "".to_string(),
            footer: "".to_string(),
            buttons: vec![],
            default_button: 0,
            radio_buttons: vec![],
            default_radio_buttons: 0,
            main_icon: TASKDIALOGCONFIG_0 { hMainIcon: HICON(null_mut()) },
            footer_icon: TASKDIALOGCONFIG_1 { hFooterIcon: HICON(null_mut()) },
            dialog_hwnd: HWND(null_mut()),
            is_destroyed: false,
            hyperlink_callback: None,
            callback: None,
            cx_width: 0,
            progress: ProgressState::None,
            x_dialog_id: 0,
            open_dialogs,
        }
    }
}

impl TaskDialogConfig {
    /**
    Add TDF_SHOW_PROGRESS_BAR flag on marquee is false;

    Add TDF_SHOW_MARQUEE_PROGRESS_BAR flag on marquee is true;

    https://docs.microsoft.com/en-us/windows/win32/controls/progress-bar-control
    */
    // pub fn enable_progress_bar(&mut self, marquee: bool) {
    //     if marquee {
    //         if self.flags & TDF_SHOW_MARQUEE_PROGRESS_BAR != TDF_SHOW_MARQUEE_PROGRESS_BAR {
    //             self.flags |= TDF_SHOW_MARQUEE_PROGRESS_BAR;
    //         }
    //     } else {
    //         if self.flags & TDF_SHOW_PROGRESS_BAR != TDF_SHOW_PROGRESS_BAR {
    //             self.flags |= TDF_SHOW_PROGRESS_BAR;
    //         }
    //     }
    // }

    // /** disables progress bar */
    // pub fn disable_progress_bar(&mut self, marquee: bool) {
    //     if marquee {
    //         if self.flags & TDF_SHOW_MARQUEE_PROGRESS_BAR == TDF_SHOW_MARQUEE_PROGRESS_BAR {
    //             self.flags &= !TDF_SHOW_MARQUEE_PROGRESS_BAR;
    //         }
    //     } else {
    //         if self.flags & TDF_SHOW_PROGRESS_BAR == TDF_SHOW_PROGRESS_BAR {
    //             self.flags &= !TDF_SHOW_PROGRESS_BAR;
    //         }
    //     }
    // }

    /** Set status or animation time of marquee progress bar */
    pub fn set_progress_bar_marquee_on_off(&mut self, enable: bool) {
        if self.dialog_hwnd.is_invalid() {
            return;
        }
        unsafe {
            let v = if enable { TRUE.0 as usize } else { FALSE.0 as usize };
            SendMessageW(self.dialog_hwnd, TDM_SET_PROGRESS_BAR_MARQUEE.0 as u32, WPARAM(v), LPARAM(0));
        }
    }

    pub fn set_progress_bar_marquee_progress(&mut self, enable: bool) {
        if self.dialog_hwnd.is_invalid() {
            return;
        }
        unsafe {
            let v = if enable { TRUE.0 as usize } else { FALSE.0 as usize };
            SendMessageW(self.dialog_hwnd, TDM_SET_MARQUEE_PROGRESS_BAR.0 as u32, WPARAM(v), LPARAM(0));
        }
    }

    /** Set the percentage of the progress bar */
    pub fn set_progress_bar_pos(&mut self, percentage: usize) {
        if self.dialog_hwnd.is_invalid() {
            return;
        }
        unsafe {
            SendMessageW(self.dialog_hwnd, TDM_SET_PROGRESS_BAR_POS.0 as u32, WPARAM(percentage), LPARAM(0));
        }
    }

    /** Set the content text */
    pub fn set_content(&mut self, content: &str) {
        if self.dialog_hwnd.is_invalid() {
            return;
        }
        self.content = content.to_string();
        unsafe {
            let content_wchar = U16CString::from_str_unchecked(content);
            SendMessageW(
                self.dialog_hwnd,
                TDM_SET_ELEMENT_TEXT.0 as u32,
                WPARAM(TDE_CONTENT.0 as usize),
                LPARAM(content_wchar.as_ptr() as isize),
            );
        }
    }

    /** Set the main instruction text */
    pub fn set_main_instruction(&mut self, main_instruction: &str) {
        if self.dialog_hwnd.is_invalid() {
            return;
        }
        self.main_instruction = main_instruction.to_string();
        unsafe {
            let main_instruction_wchar = U16CString::from_str_unchecked(main_instruction);
            SendMessageW(
                self.dialog_hwnd,
                TDM_SET_ELEMENT_TEXT.0 as u32,
                WPARAM(TDE_MAIN_INSTRUCTION.0 as usize),
                LPARAM(main_instruction_wchar.as_ptr() as isize),
            );
        }
    }

    /** Set the footer text */
    pub fn set_footer(&mut self, footer: &str) {
        if self.dialog_hwnd.is_invalid() {
            return;
        }
        self.footer = footer.to_string();
        unsafe {
            let footer_wchar = U16CString::from_str_unchecked(footer);
            SendMessageW(
                self.dialog_hwnd,
                TDM_SET_ELEMENT_TEXT.0 as u32,
                WPARAM(TDE_FOOTER.0 as usize),
                LPARAM(footer_wchar.as_ptr() as isize),
            );
        }
    }

    /** Set the expanded information text */
    pub fn set_expanded_information(&mut self, expanded_information: &str) {
        if self.dialog_hwnd.is_invalid() {
            return;
        }
        self.expanded_information = expanded_information.to_string();
        unsafe {
            let expanded_information_wchar = U16CString::from_str_unchecked(expanded_information);
            SendMessageW(
                self.dialog_hwnd,
                TDM_SET_ELEMENT_TEXT.0 as u32,
                WPARAM(TDE_EXPANDED_INFORMATION.0 as usize),
                LPARAM(expanded_information_wchar.as_ptr() as isize),
            );
        }
    }

    /** Set the button elevation state */
    pub fn set_button_elevation_required_state(&mut self, button_id: usize, enable: bool) {
        if self.dialog_hwnd.is_invalid() {
            return;
        }
        unsafe {
            SendMessageW(
                self.dialog_hwnd,
                TDM_SET_BUTTON_ELEVATION_REQUIRED_STATE.0 as u32,
                WPARAM(button_id),
                LPARAM(if enable { 1 } else { 0 }),
            );
        }
    }
}

struct TaskDialogButton {
    pub id: i32,
    pub text: String,
}

struct TaskDialogResult {
    pub button_id: i32,
    pub radio_button_id: i32,
    pub checked: bool,
}

impl Default for TaskDialogResult {
    fn default() -> Self {
        TaskDialogResult { button_id: 0, radio_button_id: 0, checked: false }
    }
}

unsafe fn execute_task_dialog(conf: &mut TaskDialogConfig) -> Result<TaskDialogResult, windows::core::Error> {
    let mut result = TaskDialogResult::default();
    let conf_ptr: *mut TaskDialogConfig = conf;
    let conf_long_ptr = conf_ptr as isize;

    // Call GetModuleHandleA on conf.instance is null
    let instance = if conf.instance.is_invalid() { GetModuleHandleW(PCWSTR(std::ptr::null())).unwrap() } else { conf.instance };

    // Some text
    let window_title: U16CString = U16CString::from_str_unchecked(&conf.window_title);
    let main_instruction: U16CString = U16CString::from_str_unchecked(&conf.main_instruction);
    let content: U16CString = U16CString::from_str_unchecked(&conf.content);
    let verification_text: U16CString = U16CString::from_str_unchecked(&conf.verification_text);
    let expanded_information: U16CString = U16CString::from_str_unchecked(&conf.expanded_information);
    let expanded_control_text: U16CString = U16CString::from_str_unchecked(&conf.expanded_control_text);
    let collapsed_control_text: U16CString = U16CString::from_str_unchecked(&conf.collapsed_control_text);
    let footer: U16CString = U16CString::from_str_unchecked(&conf.footer);

    // Buttons
    let btn_text: Vec<U16CString> = conf.buttons.iter().map(|btn| U16CString::from_str_unchecked(&btn.text)).collect();
    let buttons: Vec<TASKDIALOG_BUTTON> = conf
        .buttons
        .iter()
        .enumerate()
        .map(|(i, btn)| TASKDIALOG_BUTTON { nButtonID: btn.id, pszButtonText: PCWSTR(btn_text[i].as_ptr()) })
        .collect();

    // Radio Buttons
    let radio_btn_text: Vec<U16CString> = conf.radio_buttons.iter().map(|btn| U16CString::from_str_unchecked(&btn.text)).collect();
    let radio_buttons: Vec<TASKDIALOG_BUTTON> = conf
        .radio_buttons
        .iter()
        .enumerate()
        .map(|(i, btn)| TASKDIALOG_BUTTON { nButtonID: btn.id, pszButtonText: PCWSTR(radio_btn_text[i].as_ptr()) })
        .collect();

    // ICON
    unsafe extern "system" fn callback(
        hwnd: HWND,
        msg: TASKDIALOG_NOTIFICATIONS,
        _w_param: WPARAM,
        _l_param: LPARAM,
        lp_ref_data: isize,
    ) -> HRESULT {
        let conf = std::mem::transmute::<isize, *mut TaskDialogConfig>(lp_ref_data);
        match msg {
            TDN_CREATED => {
                (*conf).dialog_hwnd = hwnd;
            }
            TDN_DESTROYED => {
                (*conf).is_destroyed = true;
            }
            TDN_HYPERLINK_CLICKED => {
                let link = U16CString::from_ptr_str(_l_param.0 as *const u16).to_string().unwrap();
                if let Some(callback) = (*conf).hyperlink_callback {
                    callback(&link);
                }
            }
            _ => {}
        };
        if let Some(callback) = (*conf).callback {
            return callback(hwnd, msg, _w_param, _l_param, lp_ref_data as _);
        }
        S_OK
    }

    let config = TASKDIALOGCONFIG {
        cbSize: std::mem::size_of::<TASKDIALOGCONFIG>() as u32,
        hwndParent: conf.parent,
        hInstance: instance.into(),
        dwFlags: conf.flags,
        dwCommonButtons: conf.common_buttons,
        pszWindowTitle: PCWSTR(window_title.as_ptr()),
        pszMainInstruction: PCWSTR(main_instruction.as_ptr()),
        pszContent: PCWSTR(content.as_ptr()),
        pszVerificationText: PCWSTR(verification_text.as_ptr()),
        pszExpandedInformation: PCWSTR(expanded_information.as_ptr()),
        pszExpandedControlText: PCWSTR(expanded_control_text.as_ptr()),
        pszCollapsedControlText: PCWSTR(collapsed_control_text.as_ptr()),
        pszFooter: PCWSTR(footer.as_ptr()),
        cButtons: buttons.len() as u32,
        pButtons: buttons.as_slice().as_ptr(),
        nDefaultButton: conf.default_button,
        cRadioButtons: radio_buttons.len() as u32,
        pRadioButtons: radio_buttons.as_slice().as_ptr(),
        nDefaultRadioButton: conf.default_radio_buttons,
        Anonymous1: conf.main_icon,
        Anonymous2: conf.footer_icon,
        pfCallback: Some(callback),
        lpCallbackData: conf_long_ptr,
        cxWidth: conf.cx_width,
    };

    let mut verify: BOOL = FALSE;
    let dialog_result = TaskDialogIndirect(&config, Some(&mut result.button_id), Some(&mut result.radio_button_id), Some(&mut verify));
    result.checked = verify != BOOL(0);
    dialog_result?;
    Ok(result)
}
