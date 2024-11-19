use serde_derive::{Deserialize, Serialize};

fn main() {
    xdialog::XDialogBuilder::new().run_loop(run);
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
                <span>hello world</span>
                <textarea></textarea>
                <button onclick="window.external.invoke('something');">click me</button>
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
    options.resizable = false;

    let view = xdialog::show_webview(options).unwrap();

    std::thread::sleep_ms(10000);

    // let mut webview = web_view::builder()
    //     .title("Rust Todo App")
    //     .content(Content::Html(html))
    //     .size(320, 480)
    //     .resizable(false)
    //     .debug(true)
    //     .user_data(vec![])
    //     .invoke_handler(|webview, arg| {
    //         use Cmd::*;

    //         let tasks_len = {
    //             let tasks = webview.user_data_mut();

    //             match serde_json::from_str(arg).unwrap() {
    //                 Init => (),
    //                 Log { text } => println!("{}", text),
    //                 AddTask { name } => tasks.push(Task { name, done: false }),
    //                 MarkTask { index, done } => tasks[index].done = done,
    //                 ClearDoneTasks => tasks.retain(|t| !t.done),
    //             }

    //             tasks.len()
    //         };

    //         webview.set_title(&format!("Rust Todo App ({} Tasks)", tasks_len))?;
    //         render(webview)
    //     })
    //     .build()
    //     .unwrap();

    // webview.set_color((156, 39, 176));

    // let res = webview.run().unwrap();

    // println!("final state: {:?}", res);
    0
}

// fn render(webview: &mut WebView<Vec<Task>>) -> WVResult {
//     let render_tasks = {
//         let tasks = webview.user_data();
//         println!("{:#?}", tasks);
//         format!("rpc.render({})", serde_json::to_string(tasks).unwrap())
//     };
//     webview.eval(&render_tasks)
// }

// #[derive(Debug, Serialize, Deserialize)]
// struct Task {
//     name: String,
//     done: bool,
// }

// #[derive(Deserialize)]
// #[serde(tag = "cmd", rename_all = "camelCase")]
// pub enum Cmd {
//     Init,
//     Log { text: String },
//     AddTask { name: String },
//     MarkTask { index: usize, done: bool },
//     ClearDoneTasks,
// }

fn inline_style(s: &str) -> String {
    format!(r#"<style type="text/css">{}</style>"#, s)
}

fn inline_script(s: &str) -> String {
    format!(r#"<script type="text/javascript">{}</script>"#, s)
}
