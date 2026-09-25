//! Win32 TaskDialog. Each dialog runs `TaskDialogIndirect` on its own thread and applies the
//! requests for it on the dialog's timer (`TDN_TIMER`).

use std::collections::HashMap;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex, MutexGuard};

use windows::core::{HRESULT, HSTRING, PCWSTR};
use windows::Win32::Foundation::{FALSE, HWND, LPARAM, S_FALSE, S_OK, TRUE, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Controls::{
    TaskDialogIndirect, TASKDIALOGCONFIG, TASKDIALOGCONFIG_0, TASKDIALOG_BUTTON, TASKDIALOG_NOTIFICATIONS, TDE_CONTENT,
    TDF_CALLBACK_TIMER, TDF_SHOW_PROGRESS_BAR, TDF_SIZE_TO_CONTENT, TDM_SET_ELEMENT_TEXT, TDM_SET_MARQUEE_PROGRESS_BAR,
    TDM_SET_PROGRESS_BAR_MARQUEE, TDM_SET_PROGRESS_BAR_POS, TDN_BUTTON_CLICKED, TDN_CREATED, TDN_TIMER, TD_ERROR_ICON,
    TD_INFORMATION_ICON, TD_WARNING_ICON,
};
use windows::Win32::UI::WindowsAndMessaging::{EndDialog, SendMessageW};

use crate::channel::DialogRequestHandler;
use crate::model::{CreationSender, DialogMessageRequest};
use crate::{ProgressButtonCallback, ProgressDialogProxy, XDialogError, XDialogIcon, XDialogOptions, XDialogResult};

/// Manages Win32 Task Dialogs. TaskDialogs need no event loop, so the manager serves requests
/// itself: as the installed handler (builder `Win32`, `init_win32_direct`) and for the egui
/// runtime's Win32 routing / fallback. `ExitEventLoop` closes every TaskDialog; unknown ids are
/// ignored.
pub(crate) struct TaskDialogManager {
    /// The request channel of each open dialog.
    open_dialogs: Arc<Mutex<HashMap<usize, Sender<DialogMessageRequest>>>>,
}

impl TaskDialogManager {
    pub(crate) fn new() -> Self {
        TaskDialogManager { open_dialogs: Arc::new(Mutex::new(HashMap::new())) }
    }

    fn lock(&self) -> MutexGuard<'_, HashMap<usize, Sender<DialogMessageRequest>>> {
        self.open_dialogs.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub(crate) fn show(&self, id: usize, data: XDialogOptions, has_progress: bool, creation: CreationSender, button_callback: Option<ProgressButtonCallback>) {
        let (tx, rx) = channel();
        self.lock().insert(id, tx);
        let open_dialogs = self.open_dialogs.clone();

        let (dialog_sender, dialog_receiver) = channel();
        let _ = creation.send(Ok(dialog_receiver));
        std::thread::spawn(move || {
            let mut config = TaskDialogConfig { options: data,
                                                progress: has_progress,
                                                dialog_hwnd: HWND::default(),
                                                marquee: false,
                                                x_dialog_id: id,
                                                rx,
                                                button_callback };

            let result = unsafe { execute_task_dialog(&mut config) };

            open_dialogs.lock().unwrap_or_else(|e| e.into_inner()).remove(&id);

            let xresult = match result {
                Ok(button_id) if button_id >= 0 => XDialogResult::ButtonPressed(button_id as usize),
                _ => XDialogResult::WindowClosed,
            };
            let _ = dialog_sender.send(xresult);
        });
    }

    /// Close every open dialog.
    pub(crate) fn close_all(&self) {
        for (&id, tx) in self.lock().iter() {
            let _ = tx.send(DialogMessageRequest::CloseWindow(id));
        }
    }
}

impl DialogRequestHandler for TaskDialogManager {
    fn send(&self, message: DialogMessageRequest) -> Result<(), XDialogError> {
        match message {
            DialogMessageRequest::None => {}
            DialogMessageRequest::ExitEventLoop => self.close_all(),
            DialogMessageRequest::ShowMessageWindow(id, options, result) => self.show(id, options, false, result, None),
            DialogMessageRequest::ShowProgressWindow(id, options, result, on_button) => self.show(id, options, true, result, on_button),
            DialogMessageRequest::CloseWindow(id)
            | DialogMessageRequest::SetProgressIndeterminate(id)
            | DialogMessageRequest::SetProgressValue(id, _)
            | DialogMessageRequest::SetProgressText(id, _) => {
                if let Some(tx) = self.lock().get(&id) {
                    let _ = tx.send(message);
                }
            }
        }
        Ok(())
    }
}

/// Initialize xdialog to use Win32 TaskDialog directly, without an event loop or
/// [`XDialogBuilder`](crate::XDialogBuilder). This must be called before any dialog functions.
/// Can only be called once; subsequent calls will be ignored with a warning.
#[cfg(feature = "win32-direct")]
pub fn init_win32_direct() {
    crate::channel::init_handler(Box::new(TaskDialogManager::new()));
}

fn convert_icon(icon: &XDialogIcon) -> TASKDIALOGCONFIG_0 {
    match icon {
        XDialogIcon::None => TASKDIALOGCONFIG_0::default(),
        XDialogIcon::Error => TASKDIALOGCONFIG_0 { pszMainIcon: TD_ERROR_ICON },
        XDialogIcon::Warning => TASKDIALOGCONFIG_0 { pszMainIcon: TD_WARNING_ICON },
        XDialogIcon::Information => TASKDIALOGCONFIG_0 { pszMainIcon: TD_INFORMATION_ICON },
    }
}

struct TaskDialogConfig {
    options: XDialogOptions,
    /// Show a progress bar.
    progress: bool,
    /// Set on `TDN_CREATED`.
    dialog_hwnd: HWND,
    /// The progress bar is in marquee (indeterminate) mode.
    marquee: bool,
    x_dialog_id: usize,
    /// Requests for this dialog, applied on each timer tick.
    rx: Receiver<DialogMessageRequest>,
    /// Optional callback invoked when a button is clicked. Returns `true` to keep the dialog open
    /// (returns `S_FALSE` to the task dialog) or `false` to allow it to close.
    button_callback: Option<ProgressButtonCallback>,
}

impl TaskDialogConfig {
    fn send_message(&self, msg: i32, w_param: usize, l_param: isize) {
        if self.dialog_hwnd.is_invalid() {
            return;
        }
        // SAFETY: `dialog_hwnd` is the live task dialog (messages are sent from its callback).
        unsafe {
            SendMessageW(self.dialog_hwnd, msg as u32, Some(WPARAM(w_param)), Some(LPARAM(l_param)));
        }
    }

    /// Marquee on/off: the style (`TDM_SET_MARQUEE_PROGRESS_BAR`) and the animation.
    fn set_marquee(&self, enable: bool) {
        let v = if enable { TRUE.0 as usize } else { FALSE.0 as usize };
        self.send_message(TDM_SET_PROGRESS_BAR_MARQUEE.0, v, 0);
        self.send_message(TDM_SET_MARQUEE_PROGRESS_BAR.0, v, 0);
    }

    /// Handle a task dialog notification: button callbacks, and queued requests on each timer tick.
    fn on_notification(&mut self, hwnd: HWND, msg: TASKDIALOG_NOTIFICATIONS, w_param: WPARAM) -> HRESULT {
        if msg == TDN_BUTTON_CLICKED {
            if let Some(cb) = self.button_callback.as_mut() {
                let proxy = ProgressDialogProxy::non_owning(self.x_dialog_id);
                return if cb(w_param.0, &proxy) { S_FALSE } else { S_OK };
            }
        } else if msg == TDN_TIMER {
            while let Ok(request) = self.rx.try_recv() {
                match request {
                    DialogMessageRequest::CloseWindow(_) => unsafe {
                        let _ = EndDialog(hwnd, -1);
                    },
                    DialogMessageRequest::SetProgressValue(_, progress) => {
                        if std::mem::take(&mut self.marquee) {
                            self.set_marquee(false);
                        }
                        self.send_message(TDM_SET_PROGRESS_BAR_POS.0, (progress * 100f32) as usize, 0);
                    }
                    DialogMessageRequest::SetProgressIndeterminate(_) => {
                        self.set_marquee(true);
                        self.marquee = true;
                    }
                    DialogMessageRequest::SetProgressText(_, text) => {
                        let text = HSTRING::from(text);
                        self.send_message(TDM_SET_ELEMENT_TEXT.0, TDE_CONTENT.0 as usize, text.as_ptr() as isize);
                    }
                    _ => {}
                }
            }
        }
        S_OK
    }
}

/// Run the task dialog (blocks until it closes); returns the pressed button id (`-1`: closed).
unsafe fn execute_task_dialog(conf: &mut TaskDialogConfig) -> Result<i32, windows::core::Error> {
    unsafe extern "system" fn callback(hwnd: HWND, msg: TASKDIALOG_NOTIFICATIONS, w_param: WPARAM, _l_param: LPARAM, lp_ref_data: isize) -> HRESULT {
        // SAFETY: `lp_ref_data` is the `&mut TaskDialogConfig` passed below, alive for the whole
        // `TaskDialogIndirect` call and not otherwise used during it.
        let conf = unsafe { &mut *std::ptr::with_exposed_provenance_mut::<TaskDialogConfig>(lp_ref_data as usize) };
        if msg == TDN_CREATED {
            conf.dialog_hwnd = hwnd;
        }
        conf.on_notification(hwnd, msg, w_param)
    }

    let o = &conf.options;
    let (window_title, main_instruction, content) = (HSTRING::from(&o.title), HSTRING::from(&o.main_instruction), HSTRING::from(&o.message));
    // Last button first: it is the default.
    let texts: Vec<HSTRING> = o.buttons.iter().rev().map(HSTRING::from).collect();
    let buttons: Vec<TASKDIALOG_BUTTON> = texts.iter()
                                               .zip((0..o.buttons.len() as i32).rev())
                                               .map(|(text, id)| TASKDIALOG_BUTTON { nButtonID: id, pszButtonText: PCWSTR(text.as_ptr()) })
                                               .collect();
    let mut flags = TDF_SIZE_TO_CONTENT | TDF_CALLBACK_TIMER;
    if conf.progress {
        flags |= TDF_SHOW_PROGRESS_BAR;
    }

    let config = TASKDIALOGCONFIG { cbSize: std::mem::size_of::<TASKDIALOGCONFIG>() as u32,
                                    hInstance: unsafe { GetModuleHandleW(PCWSTR::null()) }?.into(),
                                    dwFlags: flags,
                                    pszWindowTitle: PCWSTR(window_title.as_ptr()),
                                    pszMainInstruction: PCWSTR(main_instruction.as_ptr()),
                                    pszContent: PCWSTR(content.as_ptr()),
                                    cButtons: buttons.len() as u32,
                                    pButtons: buttons.as_ptr(),
                                    nDefaultButton: buttons.first().map_or(0, |b| b.nButtonID),
                                    Anonymous1: convert_icon(&o.icon),
                                    pfCallback: Some(callback),
                                    lpCallbackData: (conf as *mut TaskDialogConfig).expose_provenance() as isize,
                                    ..Default::default() };

    let mut button_id = 0;
    unsafe { TaskDialogIndirect(&config, Some(&mut button_id), None, None) }?;
    Ok(button_id)
}
