//! 浮舱这个宿主给程序舱提供的能力。
//!
//! 现在几乎全是「还没接」。这是**故意的**：明确的 `capability_unavailable` 比一个
//! 半通不通的实现好 —— 后者会让程序舱作者以为自己写对了，直到某天发现结果一直是空的。
//!
//! 接的时候一条一条接，每接一条就有一个程序舱能真的跑起来。

use podapp_runtime::HostBridge;
use serde_json::Value;

pub struct DockHost;

impl HostBridge for DockHost {
    fn ai_image_edit(&self, _a: &Value) -> Result<Value, String> {
        Err("capability_unavailable: 浮舱还没接 AI 图像编辑（第一批 Pod 用不到，接了再开）".into())
    }
    fn ai_image_generate(&self, _a: &Value) -> Result<Value, String> {
        Err("capability_unavailable: 浮舱还没接 AI 图像生成".into())
    }
    fn ai_chat(&self, _a: &Value) -> Result<Value, String> {
        Err("capability_unavailable: 浮舱还没接对话模型".into())
    }
    fn file_save(&self, _n: &str, _d: &str) -> Result<Value, String> {
        Err("capability_unavailable: 另存为对话框还没接".into())
    }
    fn file_open(&self, _f: &[String]) -> Result<Value, String> {
        Err("capability_unavailable: 打开对话框还没接".into())
    }
    fn host_action(&self, id: &str, _input: Value) -> Result<Value, String> {
        // 浮舱自己还没有任何宿主动作。九宫格切图要的 `host.zip.pack` 就装在这里。
        Err(format!("capability_unavailable: 浮舱没有宿主动作 {id}"))
    }
}
