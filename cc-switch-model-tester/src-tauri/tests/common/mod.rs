//! 测试辅助：构造模型快照。

use cc_switch_model_tester_lib::domain::ModelSnapshot;

pub fn model_snap(id: &str) -> ModelSnapshot {
    ModelSnapshot {
        model_id: id.into(),
        display_name: None,
        base_url_override: None,
        compat: serde_json::Value::Null,
        thinking_level_map: None,
        id_markers: Vec::new(),
    }
}
