//! 对真机上的 Codex 会话跑一遍解析。**只读，且只打统计不打内容** ——
//! 验证解析器扛不扛得住真实数据，不是把用户的对话倒出来。
fn main() {
    let all = podapp_codex::list_sessions(500);
    println!("解析出 {} 个会话", all.len());
    let empty = all.iter().filter(|s| s.turns == 0).count();
    let no_title = all.iter().filter(|s| s.title.starts_with('(')).count();
    println!("  其中 0 轮对话的: {empty}，取不到标题的: {no_title}");
    if let Some(s) = all.first() {
        println!(
            "最新一个: 轮数={} 标题长度={} 字",
            s.turns,
            s.title.chars().count()
        );
        match podapp_codex::read_session(&s.id) {
            Ok(v) => println!("  读取成功，消息 {} 条", v["count"]),
            Err(e) => println!("  读取失败: {e}"),
        }
    }
}
