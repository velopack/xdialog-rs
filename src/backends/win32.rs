//! Win32 TaskDialog. Each dialog runs `TaskDialogIndirect` on its own thread and applies the
//! requests for it on the dialog's timer (`TDN_TIMER`).

use std::collections::HashMap;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};

use windows::core::{s, w, BOOL, HRESULT, HSTRING, PCWSTR};
use windows::Win32::Foundation::{FreeLibrary, HANDLE, HMODULE, HWND, INVALID_HANDLE_VALUE, LPARAM, S_FALSE, S_OK, WPARAM};
use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
use windows::Win32::UI::Controls::{
    TASKDIALOGCONFIG, TASKDIALOGCONFIG_0, TASKDIALOG_BUTTON, TASKDIALOG_NOTIFICATIONS, TDE_CONTENT, TDF_CALLBACK_TIMER,
    TDF_SHOW_PROGRESS_BAR, TDF_SIZE_TO_CONTENT, TDM_SET_ELEMENT_TEXT, TDM_SET_MARQUEE_PROGRESS_BAR, TDM_SET_PROGRESS_BAR_MARQUEE,
    TDM_SET_PROGRESS_BAR_POS, TDN_BUTTON_CLICKED, TDN_CREATED, TDN_TIMER, TD_ERROR_ICON, TD_INFORMATION_ICON, TD_WARNING_ICON,
};
use windows::Win32::UI::WindowsAndMessaging::{EndDialog, SendMessageW, IDOK};

use crate::channel::DialogRequestHandler;
use crate::model::{DialogMessageRequest, DialogReply, ResultSender};
use crate::{ProgressButtonCallback, ProgressDialogProxy, XDialogError, XDialogIcon, XDialogOptions, XDialogResult};

/// Manages Win32 Task Dialogs. TaskDialogs need no event loop, so the manager serves requests
/// itself: as the installed handler (builder `Win32`, `init_win32_direct`) and for the drawn
/// dialogs' runtime (Win32 routing / fallback). `ExitEventLoop` closes every TaskDialog; unknown
/// ids are ignored.
pub(crate) struct TaskDialogManager {
    open_dialogs: Arc<Mutex<HashMap<usize, Sender<DialogMessageRequest>>>>,
}

impl TaskDialogManager {
    pub(crate) fn new() -> Self {
        TaskDialogManager { open_dialogs: Arc::new(Mutex::new(HashMap::new())) }
    }

    fn lock(&self) -> MutexGuard<'_, HashMap<usize, Sender<DialogMessageRequest>>> {
        self.open_dialogs.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub(crate) fn show(&self, id: usize, data: XDialogOptions, has_progress: bool, reply: DialogReply, button_callback: Option<ProgressButtonCallback>) {
        // Test builds: no window (the fallback tests; a real TaskDialog would take focus). A message
        // box answers at once with its default (last) button, a progress dialog stays open until closed.
        let stub = crate::backends::gui::appearance::test_flag("XDIALOG_TEST_STUB_TASKDIALOG");
        if stub && !has_progress {
            let n = data.buttons.len();
            reply.opened().send(result_from_id(n.checked_sub(1).map_or(IDOK.0, button_id), n));
            return;
        }
        let (tx, rx) = channel();
        self.lock().insert(id, tx);
        let open_dialogs = self.open_dialogs.clone();

        std::thread::spawn(move || {
            if stub {
                reply.opened();
                while !matches!(rx.recv(), Ok(DialogMessageRequest::CloseWindow(_)) | Err(_)) {}
                open_dialogs.lock().unwrap_or_else(|e| e.into_inner()).remove(&id);
                return;
            }
            let mut config = TaskDialogConfig { options: data,
                                                progress: has_progress,
                                                marquee: false,
                                                x_dialog_id: id,
                                                rx,
                                                button_callback,
                                                reply: Some(reply),
                                                sender: None };

            let result = unsafe { execute_task_dialog(&mut config) };

            open_dialogs.lock().unwrap_or_else(|e| e.into_inner()).remove(&id);

            let button_id = result.unwrap_or_else(|e| {
                                      error!("xdialog: TaskDialog {id}: {e}");
                                      if let Some(reply) = config.reply.take() {
                                          reply.failed(XDialogError::SystemError(e));
                                      }
                                      -1
                                  });
            // `TDN_CREATED` precedes every successful return; opening here is only a safety net.
            if let Some(sender) = config.sender.take().or_else(|| config.reply.take().map(DialogReply::opened)) {
                sender.send(result_from_id(button_id, config.options.buttons.len()));
            }
        });
    }

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
/// [`XDialogBuilder`](crate::XDialogBuilder). Call before any dialog function; later calls are
/// ignored with a warning.
///
/// TaskDialog is part of Common Controls v6. The executable needs no manifest for it: without
/// one that selects v6, xdialog activates v6 for its own dialogs.
#[cfg(feature = "win32-direct")]
pub fn init_win32_direct() {
    crate::channel::init_handler(Box::new(TaskDialogManager::new()));
}

fn convert_icon(icon: &XDialogIcon) -> TASKDIALOGCONFIG_0 {
    match icon {
        // `Custom` is only shown by the drawn backends (Fluent, Ubuntu, MacOS).
        XDialogIcon::None | XDialogIcon::Custom => TASKDIALOGCONFIG_0::default(),
        XDialogIcon::Error => TASKDIALOGCONFIG_0 { pszMainIcon: TD_ERROR_ICON },
        XDialogIcon::Warning => TASKDIALOGCONFIG_0 { pszMainIcon: TD_WARNING_ICON },
        XDialogIcon::Information => TASKDIALOGCONFIG_0 { pszMainIcon: TD_INFORMATION_ICON },
    }
}

/// TaskDialog's id for button `index`: clear of the standard ids (`IDOK` = 1 .. `IDCONTINUE` = 11),
/// which TaskDialog gives meaning (`IDCANCEL`: Esc and the close button).
fn button_id(index: usize) -> i32 {
    1000 + index as i32
}

/// The button (of `n`) behind TaskDialog's button id `id`, if it is one of ours.
fn button_index(id: i32, n: usize) -> Option<usize> {
    id.checked_sub(button_id(0)).and_then(|i| usize::try_from(i).ok()).filter(|&i| i < n)
}

/// The result for TaskDialog's button id `id`: anything but one of our `n` buttons (`-1` from
/// `EndDialog`, the `IDOK` TaskDialog adds when there are no buttons) is a closed window.
fn result_from_id(id: i32, n: usize) -> XDialogResult {
    button_index(id, n).map_or(XDialogResult::WindowClosed, XDialogResult::ButtonPressed)
}

struct TaskDialogConfig {
    options: XDialogOptions,
    progress: bool,
    /// The progress bar is in marquee (indeterminate) mode.
    marquee: bool,
    x_dialog_id: usize,
    /// Requests for this dialog, applied on each timer tick.
    rx: Receiver<DialogMessageRequest>,
    /// On button click: `true` keeps the dialog open (`S_FALSE` to the task dialog), `false` lets
    /// it close.
    button_callback: Option<ProgressButtonCallback>,
    /// Opened on `TDN_CREATED`; still here if the dialog never opened.
    reply: Option<DialogReply>,
    /// Where the result goes once the dialog is open.
    sender: Option<ResultSender>,
}

fn send_message(hwnd: HWND, msg: i32, w_param: usize, l_param: isize) {
    // SAFETY: `hwnd` is the live task dialog (messages are sent from its callback).
    unsafe {
        SendMessageW(hwnd, msg as u32, Some(WPARAM(w_param)), Some(LPARAM(l_param)));
    }
}

/// Marquee on/off: the style (`TDM_SET_MARQUEE_PROGRESS_BAR`) is on whenever the animation runs.
fn set_marquee(hwnd: HWND, enable: bool) {
    let (style, animation) = (TDM_SET_MARQUEE_PROGRESS_BAR.0, TDM_SET_PROGRESS_BAR_MARQUEE.0);
    for msg in if enable { [style, animation] } else { [animation, style] } {
        send_message(hwnd, msg, enable as usize, 0);
    }
}

impl TaskDialogConfig {
    /// Handle a task dialog notification: button callbacks, and queued requests on each timer tick.
    fn on_notification(&mut self, hwnd: HWND, msg: TASKDIALOG_NOTIFICATIONS, w_param: WPARAM) -> HRESULT {
        if msg == TDN_CREATED {
            self.sender = self.reply.take().map(DialogReply::opened);
        } else if msg == TDN_BUTTON_CLICKED {
            let index = button_index(w_param.0 as i32, self.options.buttons.len());
            if let (Some(index), Some(cb)) = (index, self.button_callback.as_mut()) {
                let proxy = ProgressDialogProxy::non_owning(self.x_dialog_id);
                // Unwinding into TaskDialog's frames would abort the process.
                match catch_unwind(AssertUnwindSafe(|| cb(index, &proxy))) {
                    Ok(true) => return S_FALSE,
                    Ok(false) => {}
                    Err(_) => {
                        error!("xdialog: a progress button callback panicked; closing the dialog");
                        self.button_callback = None;
                    }
                }
            }
        } else if msg == TDN_TIMER {
            while let Ok(request) = self.rx.try_recv() {
                match request {
                    DialogMessageRequest::CloseWindow(_) => unsafe {
                        let _ = EndDialog(hwnd, -1);
                    },
                    DialogMessageRequest::SetProgressValue(_, progress) => {
                        if std::mem::take(&mut self.marquee) {
                            set_marquee(hwnd, false);
                        }
                        send_message(hwnd, TDM_SET_PROGRESS_BAR_POS.0, (progress * 100f32) as usize, 0);
                    }
                    DialogMessageRequest::SetProgressIndeterminate(_) => {
                        set_marquee(hwnd, true);
                        self.marquee = true;
                    }
                    DialogMessageRequest::SetProgressText(_, text) => {
                        let text = HSTRING::from(text);
                        send_message(hwnd, TDM_SET_ELEMENT_TEXT.0, TDE_CONTENT.0 as usize, text.as_ptr() as isize);
                    }
                    _ => {}
                }
            }
        }
        S_OK
    }
}

type TaskDialogIndirectFn = unsafe extern "system" fn(*const TASKDIALOGCONFIG, *mut i32, *mut i32, *mut BOOL) -> HRESULT;

/// `TaskDialogIndirect` exists only in Common Controls v6, which a process gets from its manifest
/// (otherwise `comctl32.dll` is v5, without it). So it is looked up at run time (a static import
/// would keep an executable without such a manifest from starting), if need be under an
/// activation context of xdialog's own that selects v6.
struct Comctl6 {
    task_dialog_indirect: TaskDialogIndirectFn,
    /// The activation context to activate around each dialog (`None`: the process default
    /// selects v6 already).
    act_ctx: Option<usize>,
}

/// Resolved once per process.
fn comctl6() -> Result<&'static Comctl6, String> {
    static COMCTL6: OnceLock<Result<Comctl6, String>> = OnceLock::new();
    COMCTL6.get_or_init(load_comctl6).as_ref().map_err(Clone::clone)
}

fn load_comctl6() -> Result<Comctl6, String> {
    if let Some(task_dialog_indirect) = find_task_dialog_indirect() {
        return Ok(Comctl6 { task_dialog_indirect, act_ctx: None });
    }
    let act_ctx = create_comctl6_context()?;
    let _active = ActiveContext::new(act_ctx)?;
    let task_dialog_indirect =
        find_task_dialog_indirect().ok_or("xdialog: Common Controls v6 has no TaskDialogIndirect (Windows Vista or later is needed)")?;
    Ok(Comctl6 { task_dialog_indirect, act_ctx: Some(act_ctx) })
}

/// `TaskDialogIndirect` from the `comctl32.dll` the thread's activation context selects.
fn find_task_dialog_indirect() -> Option<TaskDialogIndirectFn> {
    // SAFETY: plain loader calls; the module stays loaded for as long as the function is used.
    unsafe {
        let module = LoadLibraryW(w!("comctl32.dll")).ok()?;
        let Some(f) = GetProcAddress(module, s!("TaskDialogIndirect")) else {
            let _ = FreeLibrary(module);
            return None;
        };
        // SAFETY: the documented signature of `TaskDialogIndirect`.
        Some(std::mem::transmute::<unsafe extern "system" fn() -> isize, TaskDialogIndirectFn>(f))
    }
}

/// `ACTCTXW`.
#[repr(C)]
struct ActCtx {
    size: u32,
    flags: u32,
    source: PCWSTR,
    processor_architecture: u16,
    lang_id: u16,
    assembly_directory: PCWSTR,
    resource_name: PCWSTR,
    application_name: PCWSTR,
    module: HMODULE,
}

windows::core::link!("kernel32.dll" "system" fn CreateActCtxW(actctx: *const ActCtx) -> HANDLE);
windows::core::link!("kernel32.dll" "system" fn ActivateActCtx(actctx: HANDLE, cookie: *mut usize) -> BOOL);
windows::core::link!("kernel32.dll" "system" fn DeactivateActCtx(flags: u32, cookie: usize) -> BOOL);

const COMCTL6_MANIFEST: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <dependency>
    <dependentAssembly>
      <assemblyIdentity type="win32" name="Microsoft.Windows.Common-Controls" version="6.0.0.0" processorArchitecture="*"
                        publicKeyToken="6595b64144ccf1df" language="*"/>
    </dependentAssembly>
  </dependency>
</assembly>
"#;

/// An activation context that selects Common Controls v6, never released. `CreateActCtxW` reads
/// its manifest from a file or from a module's resources, and xdialog is no module of its own:
/// the manifest goes through a temporary file (as WinForms' `EnableVisualStyles` does).
fn create_comctl6_context() -> Result<usize, String> {
    static CALLS: AtomicUsize = AtomicUsize::new(0);
    let call = CALLS.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!("xdialog-comctl6-{}-{call}.manifest", std::process::id()));
    std::fs::write(&path, COMCTL6_MANIFEST).map_err(|e| format!("xdialog: could not write {}: {e}", path.display()))?;
    let source = HSTRING::from(path.as_path());
    let ctx = ActCtx { size: std::mem::size_of::<ActCtx>() as u32,
                       flags: 0,
                       source: PCWSTR(source.as_ptr()),
                       processor_architecture: 0,
                       lang_id: 0,
                       assembly_directory: PCWSTR::null(),
                       resource_name: PCWSTR::null(),
                       application_name: PCWSTR::null(),
                       module: HMODULE::default() };
    // SAFETY: `ctx` and the strings it points to outlive the call.
    let handle = unsafe { CreateActCtxW(&ctx) };
    let error = std::io::Error::last_os_error();
    let _ = std::fs::remove_file(&path);
    if handle == INVALID_HANDLE_VALUE {
        return Err(format!("xdialog: no Common Controls v6 activation context: {error}"));
    }
    Ok(handle.0 as usize)
}

/// An activation context active on this thread until dropped (on the same thread, innermost first).
struct ActiveContext(usize);

impl ActiveContext {
    fn new(act_ctx: usize) -> Result<Self, String> {
        let mut cookie = 0;
        // SAFETY: `act_ctx` is a live context (never released).
        if unsafe { ActivateActCtx(HANDLE(act_ctx as _), &mut cookie) }.as_bool() {
            Ok(ActiveContext(cookie))
        } else {
            Err(format!("xdialog: could not activate Common Controls v6: {}", std::io::Error::last_os_error()))
        }
    }
}

impl Drop for ActiveContext {
    fn drop(&mut self) {
        // SAFETY: the cookie of this thread's innermost activation.
        let _ = unsafe { DeactivateActCtx(0, self.0) };
    }
}

/// Run the task dialog (blocks until it closes); returns TaskDialog's button id (`-1`: closed).
unsafe fn execute_task_dialog(conf: &mut TaskDialogConfig) -> Result<i32, String> {
    unsafe extern "system" fn callback(hwnd: HWND, msg: TASKDIALOG_NOTIFICATIONS, w_param: WPARAM, _l_param: LPARAM, lp_ref_data: isize) -> HRESULT {
        // SAFETY: `lp_ref_data` is the `&mut TaskDialogConfig` passed below, alive for the whole
        // `TaskDialogIndirect` call and not otherwise used during it.
        let conf = unsafe { &mut *std::ptr::with_exposed_provenance_mut::<TaskDialogConfig>(lp_ref_data as usize) };
        conf.on_notification(hwnd, msg, w_param)
    }

    let comctl6 = comctl6()?;
    // Active for the dialog's whole life: its controls are created and drawn within it.
    let _active = comctl6.act_ctx.map(ActiveContext::new).transpose()?;

    let o = &conf.options;
    let (window_title, main_instruction, content) = (HSTRING::from(&o.title), HSTRING::from(&o.main_instruction), HSTRING::from(&o.message));
    // Last button first: it is the default.
    let texts: Vec<(i32, HSTRING)> = o.buttons.iter().enumerate().rev().map(|(i, b)| (button_id(i), HSTRING::from(b))).collect();
    let buttons: Vec<TASKDIALOG_BUTTON> = texts.iter()
                                               .map(|(id, text)| TASKDIALOG_BUTTON { nButtonID: *id, pszButtonText: PCWSTR(text.as_ptr()) })
                                               .collect();
    let mut flags = TDF_SIZE_TO_CONTENT | TDF_CALLBACK_TIMER;
    if conf.progress {
        flags |= TDF_SHOW_PROGRESS_BAR;
    }

    let config = TASKDIALOGCONFIG { cbSize: std::mem::size_of::<TASKDIALOGCONFIG>() as u32,
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
    // SAFETY: `config` and everything it points to outlive the call.
    unsafe { (comctl6.task_dialog_indirect)(&config, &mut button_id, std::ptr::null_mut(), std::ptr::null_mut()) }
        .ok()
        .map_err(|e| format!("xdialog: TaskDialogIndirect failed: {e}"))?;
    Ok(button_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::UI::WindowsAndMessaging::IDCANCEL;

    #[test]
    fn button_ids_map_back_to_results() {
        assert_eq!(result_from_id(button_id(2), 3), XDialogResult::ButtonPressed(2));
        assert_eq!(result_from_id(button_id(0), 1), XDialogResult::ButtonPressed(0));
        assert_eq!(result_from_id(button_id(3), 3), XDialogResult::WindowClosed);
        assert_eq!(result_from_id(-1, 3), XDialogResult::WindowClosed);
        assert_eq!(result_from_id(IDCANCEL.0, 3), XDialogResult::WindowClosed);
        assert_eq!(result_from_id(IDOK.0, 0), XDialogResult::WindowClosed);
        assert_eq!(result_from_id(i32::MIN, 3), XDialogResult::WindowClosed);
        assert_eq!(button_index(IDOK.0, 3), None);
    }

    #[test]
    fn task_dialog_indirect_resolves() {
        // Without a manifest (unit tests: `build.rs` adds one to integration tests only) this takes
        // the activation context, unless another test already loaded v6 into the process.
        comctl6().unwrap();
    }

    #[test]
    fn own_activation_context_selects_comctl6() {
        let act_ctx = create_comctl6_context().unwrap();
        let _active = ActiveContext::new(act_ctx).unwrap();
        assert!(find_task_dialog_indirect().is_some());
    }
}
