// Windows 发布版不要黑框：`windows_subsystem = "windows"` 只在 release 生效，
// debug 保留控制台，否则跟随线程和自检的输出就没地方看了。
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    podapp_dock_lib::run()
}
