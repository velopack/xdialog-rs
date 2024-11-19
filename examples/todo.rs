use std::sync::Mutex;

use serde_derive::{Deserialize, Serialize};
use xdialog::WebviewDialogProxy;

fn main() {
    xdialog::XDialogBuilder::new().run_loop(run);
}

lazy_static::lazy_static! {
    static ref TASKS: Mutex<Vec<Task>> = Mutex::new(vec![
        Task { name: "Learn Rust".to_string(), done: true },
        Task { name: "Write CLI tool".to_string(), done: false },
        Task { name: "Profit!".to_string(), done: false },
    ]);
}

fn run() -> i32 {
    let html = format!(
        r#"
		<!doctype html>
		<html>
			<head>
				{styles}
			</head>
			<body>
				<!--[if lt IE 9]>
				<div class="ie-upgrade-container">
					<p class="ie-upgrade-message">Please, upgrade Internet Explorer to continue using this software.</p>
					<a class="ie-upgrade-link" target="_blank" href="https://www.microsoft.com/en-us/download/internet-explorer.aspx">Upgrade</a>
				</div>
				<![endif]-->
				<!--[if gte IE 9 | !IE ]> <!-->
				{scripts}
				<![endif]-->
			</body>
		</html>
		"#,
        styles = inline_style(include_str!("todo/styles.css")),
        scripts = inline_script(include_str!("todo/picodom.js")) + &inline_script(include_str!("todo/app.js")),
    );

    let mut options = xdialog::XDialogWebviewOptions::default();
    options.title = "Rust Todo App".to_owned();
    options.html = html;
    options.size = Some((320, 480));
    options.fixed_size = true;
    options.callback = Some(|proxy, arg| {
        println!("callback invoked: {}", arg);
        use Cmd::*;
        let tasks_len = {
            // let tasks = webview.user_data_mut();
            let mut tasks = TASKS.lock().unwrap();

            match serde_json::from_str(&arg).unwrap() {
                Init => (),
                Log { text } => println!("{}", text),
                AddTask { name } => tasks.push(Task { name, done: false }),
                MarkTask { index, done } => tasks[index].done = done,
                ClearDoneTasks => tasks.retain(|t| !t.done),
            }

            tasks.len()
        };

        proxy.set_title(&format!("Rust Todo App ({} Tasks)", tasks_len)).unwrap();
        render(&proxy);
    });

    let view = xdialog::show_webview(options).unwrap();
    std::thread::sleep_ms(15000);
    0
}

fn render(webview: &WebviewDialogProxy) {
    let render_tasks = {
        let tasks = TASKS.lock().unwrap();
        println!("{:#?}", tasks);
        format!("rpc.render({})", serde_json::to_string(&*tasks).unwrap())
    };
    webview.eval_js(&render_tasks).unwrap();
}

#[derive(Debug, Serialize, Deserialize)]
struct Task {
    name: String,
    done: bool,
}

#[derive(Deserialize)]
#[serde(tag = "cmd", rename_all = "camelCase")]
pub enum Cmd {
    Init,
    Log { text: String },
    AddTask { name: String },
    MarkTask { index: usize, done: bool },
    ClearDoneTasks,
}

fn inline_style(s: &str) -> String {
    format!(r#"<style type="text/css">{}</style>"#, s)
}

fn inline_script(s: &str) -> String {
    format!(r#"<script type="text/javascript">{}</script>"#, s)
}
