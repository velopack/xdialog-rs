mod color;
mod error;
mod escape;
mod webview;

use std::{
    collections::HashMap,
    sync::mpsc::Receiver,
    thread::{self, JoinHandle},
    time::Instant,
};

use windows::{
    core::{IUnknown, Interface, IntoParam, Result},
    Win32::{
        Foundation::{BOOL, HWND, RECT},
        System::WinRT::{
            RoInitialize,
            Xaml::{IDesktopWindowXamlSourceNative, IDesktopWindowXamlSourceNative2},
            RO_INIT_SINGLETHREADED,
        },
        UI::WindowsAndMessaging::{GetClientRect, SetWindowPos, MSG, SET_WINDOW_POS_FLAGS, SWP_SHOWWINDOW},
    },
    UI::Xaml::{
        Application, Controls::{Button, Page, StackPanel, TextBlock, TextBox}, Hosting::{DesktopWindowXamlSource, WindowsXamlManager}, Markup::XamlReader, ResourceDictionary, RoutedEventHandler, UIElement
    },
};
use winit::{
    application::ApplicationHandler,
    event::{Event, StartCause, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    platform::windows::IconExtWindows,
    window::{Icon, Window, WindowId},
};

use crate::{DialogMessageRequest, XDialogTheme};

use super::XDialogBackendImpl;

const MAIN_XAML: &'static str = include_str!("main.xaml");

const RESOURCE_XAML: &'static str = include_str!("Mile.Xaml.Styles.SunValley.xaml");


pub struct WebviewBackend;

pub struct XamlIslandApp {
    pub receiver: Receiver<DialogMessageRequest>,
    pub theme: XDialogTheme,
    pub thread: JoinHandle<Box<dyn Send + 'static>>,
    pub windows: HashMap<usize, (Window, Page, DesktopWindowXamlSource, HWND)>,
}

impl ApplicationHandler for XamlIslandApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        // event_loop
        //     .create_window(Window::default_attributes())
        //     .unwrap();
        println!("Resumed");
    }

    fn new_events(&mut self, event_loop: &ActiveEventLoop, cause: StartCause) {
        loop {
            // read all messages until there are no more queued
            // println!("Checking for messages");
            let message = self.receiver.try_recv().unwrap_or(DialogMessageRequest::None);
            if message == DialogMessageRequest::None {
                break;
            }

            match message {
                DialogMessageRequest::None => {}
                DialogMessageRequest::ShowMessageWindow(id, data) => {
                    unsafe {
                        let attribues = Window::default_attributes()
                            .with_visible(false)
                            .with_title(data.title);

                        let window = event_loop.create_window(attribues).unwrap();

                        let window_id = window.id();

                        let desktop_source = DesktopWindowXamlSource::new().unwrap();
                        let idestkop_source: IDesktopWindowXamlSourceNative = desktop_source.cast().unwrap();

                        let hwnd: u64 = window_id.into();
                        let hwnd: HWND = HWND(hwnd as isize);
                        idestkop_source.AttachToWindow(hwnd).unwrap();
                        let xaml_island_hwnd = idestkop_source.WindowHandle().unwrap() as HWND;

                        let size = window.inner_size();
                        SetWindowPos(xaml_island_hwnd, HWND(0), 0, 0, size.width as _, size.height as _, SWP_SHOWWINDOW);

                        let main_page: Page = XamlReader::Load(MAIN_XAML).unwrap().cast().unwrap();

                        desktop_source.SetContent(&main_page).unwrap();

                        window.set_visible(true);

                        self.windows.insert(id, (window, main_page, desktop_source, xaml_island_hwnd));
                        println!("ShowMessageWindow: {:?}", id);
                    }

                    // let mut d = CustomFltkDialog::new(id, data, &spacing, false);
                    // d.show();
                    // dialogs2.borrow_mut().insert(id, d);
                }
                DialogMessageRequest::ExitEventLoop => {
                    self.windows.clear();
                    event_loop.exit();
                    break;
                }
                DialogMessageRequest::CloseWindow(id) => {
                    self.windows.remove(&id);

                    // if let Some(dialog) = dialogs2.borrow_mut().get_mut(&id) {
                    //     dialog.close();
                    // }
                }
                DialogMessageRequest::ShowProgressWindow(id, data) => {
                    // let mut d = CustomFltkDialog::new(id, data, &spacing, true);
                    // d.show();
                    // dialogs2.borrow_mut().insert(id, d);
                }
                DialogMessageRequest::SetProgressIndeterminate(id) => {
                    // if let Some(dialog) = dialogs2.borrow_mut().get_mut(&id) {
                    //     dialog.set_progress_indeterminate();
                    // }
                }
                DialogMessageRequest::SetProgressValue(id, value) => {
                    // if let Some(dialog) = dialogs2.borrow_mut().get_mut(&id) {
                    //     dialog.set_progress_value(value);
                    // }
                }
                DialogMessageRequest::SetProgressText(id, text) => {
                    // if let Some(dialog) = dialogs2.borrow_mut().get_mut(&id) {
                    //     dialog.set_body_text(&text);
                    // }
                }
            }
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        println!("Window event: {:?}", event);
        // let id: u64 = id.into();
        // let id = id as usize;
        match event {
            WindowEvent::Resized(size) => {
                // iterate each window and resize it
                for (_, (_, _, _, xaml_island_hwnd)) in self.windows.iter_mut() {
                    unsafe {
                        SetWindowPos(xaml_island_hwnd.clone(), HWND(0), 0, 0, size.width as _, size.height as _, SWP_SHOWWINDOW);
                    }
                }
            }
            WindowEvent::CloseRequested => {
                // self.windows.remove(&id);

                // println!("The close button was pressed; stopping");
                // event_loop.exit();
            }
            WindowEvent::RedrawRequested => {
                // if let Some(wnd) = self.windows. {
                //     wnd.request_redraw();
                // }
                // Redraw the application.
                //
                // It's preferable for applications that do not render continuously to render in
                // this event rather than in AboutToWait, since rendering in here allows
                // the program to gracefully handle redraws requested by the OS.

                // Draw.

                // Queue a RedrawRequested event.
                //
                // You only need to call this if you've determined that you need to redraw in
                // applications which do not always need to. Applications that redraw continuously
                // can render here instead.
                // self.window.as_ref().unwrap().request_redraw();
            }
            _ => (),
        }
    }
}

impl<T: Send + 'static> XDialogBackendImpl<T> for XamlIslandBackend {
    fn run(main: fn() -> T, receiver: Receiver<DialogMessageRequest>, theme: XDialogTheme) -> T {
        let _ = unsafe { RoInitialize(RO_INIT_SINGLETHREADED) };
        let _manager = WindowsXamlManager::InitializeForCurrentThread().unwrap();

        // let resource_dict: ResourceDictionary = XamlReader::Load(RESOURCE_XAML).unwrap().cast().unwrap();
        // let test = Application::Current().unwrap().Resources().unwrap();

        let event_loop = EventLoop::new().unwrap();
        event_loop.set_control_flow(ControlFlow::Poll);

        let thread: JoinHandle<Box<dyn Send + 'static>> = thread::spawn(move || {
            return Box::new(main()) as Box<dyn Send + 'static>;
        });

        let mut app = XamlIslandApp { receiver, theme, thread, windows: HashMap::new() };

        println!("Running XamlIslandBackend");

        event_loop.run_app(&mut app).unwrap();

        println!("XamlIslandBackend finished");

        todo!();

        // unsafe {
        //     let _ = RoInitialize(RO_INIT_SINGLETHREADED);

        //     let manager = WindowsXamlManager::InitializeForCurrentThread().unwrap();
        //     let desktop_source = DesktopWindowXamlSource::new().unwrap();

        //     let event_loop = EventLoop::new();
        //     let window = WindowBuilder::new().build(&event_loop).unwrap();
        //     window.set_title("XAML Island on rust");
        //     // window.set_window_icon(Some(Icon::from_resource(1, None).unwrap()));

        //     let hwnd = window.hwnd();
        //     let window_id = window.id();

        //     let xaml_island_hwnd = {
        //         let idestkop_source: IDesktopWindowXamlSourceNative =
        //             std::convert::TryFrom::try_from(&desktop_source).unwrap();
        //         idestkop_source.AttachToWindow(hwnd).unwrap();
        //         idestkop_source.WindowHandle().unwrap() as HWND
        //     };
        //     {
        //         let size = window.inner_size();
        //         unsafe {
        //             SetWindowPos(
        //                 xaml_island_hwnd,
        //                 std::ptr::null_mut(),
        //                 0,
        //                 0,
        //                 size.width as _,
        //                 size.height as _,
        //                 SWP_SHOWWINDOW,
        //             )
        //         };
        //     }

        //     let main_page: Page = XamlReader::load(MAIN_XAML).unwrap().cast().unwrap();
        //     desktop_source.set_content(&main_page).unwrap();

        //     let text_box: TextBox = main_page.find_name("text_box").unwrap().cast().unwrap();
        //     let stack: StackPanel = main_page.find_name("stack").unwrap().cast().unwrap();
        //     let button: Button = main_page.find_name("button").unwrap().cast().unwrap();
        //     button
        //         .click(RoutedEventHandler::new(move |_, _| {
        //             let children = stack.children()?;
        //             let input_text = text_box.text()?;
        //             if !input_text.is_empty() {
        //                 text_box.set_text("")?;
        //                 children.append({
        //                     let new_text = TextBlock::new()?;
        //                     new_text.set_text(input_text.clone())?;

        //                     new_text
        //                 })?;
        //             }

        //             Ok(())
        //         }))
        //         .unwrap();

        //     event_loop.run(move |event, _, control_flow| {
        //         *control_flow = ControlFlow::Wait;

        //         match event {
        //             Event::WindowEvent {
        //                 event: WindowEvent::CloseRequested,
        //                 window_id: w_id,
        //             } if w_id == window_id => *control_flow = ControlFlow::Exit,
        //             Event::WindowEvent {
        //                 event: WindowEvent::Resized(size),
        //                 window_id: w_id,
        //             } if w_id == window_id => {
        //                 unsafe {
        //                     SetWindowPos(
        //                         xaml_island_hwnd,
        //                         std::ptr::null_mut(),
        //                         0,
        //                         0,
        //                         size.width as _,
        //                         size.height as _,
        //                         SWP_SHOWWINDOW,
        //                     )
        //                 };
        //             }
        //             _ => {}
        //         }
        //     });

        //     manager.close();

        //     todo!();
        // }
    }
}
