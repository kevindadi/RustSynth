use base64::{engine::general_purpose, Engine as _};

#[test]
fn test_general_purpose_config_padding() {
    // 使用默认配置（带填充）
    let config_default = general_purpose::GeneralPurposeConfig::new();
    let engine_default = general_purpose::GeneralPurpose::new(
        &general_purpose::alphabet::STANDARD,
        config_default,
    );
    
    let data = b"hello world";
    let encoded_default = engine_default.encode(data);
    assert_eq!(encoded_default, "aGVsbG8gd29ybGQ=");
    
    // 使用 encode_padding() 获取当前填充配置
    let config_no_padding = general_purpose::GeneralPurposeConfig::new()
        .with_encode_padding(false);
    let engine_no_padding = general_purpose::GeneralPurpose::new(
        &general_purpose::alphabet::STANDARD,
        config_no_padding,
    );
    
    let encoded_no_padding = engine_no_padding.encode(data);
    assert_eq!(encoded_no_padding, "aGVsbG8gd29ybGQ");
    
    // 验证 encode_padding() 方法
    assert!(config_default.encode_padding());
    assert!(!config_no_padding.encode_padding());
    
    // 测试 with_encode_padding(true) 显式启用填充
    let config_explicit_padding = general_purpose::GeneralPurposeConfig::new()
        .with_encode_padding(true);
    let engine_explicit_padding = general_purpose::GeneralPurpose::new(
        &general_purpose::alphabet::STANDARD,
        config_explicit_padding,
    );
    
    let encoded_explicit = engine_explicit_padding.encode(data);
    assert_eq!(encoded_explicit, "aGVsbG8gd29ybGQ=");
    assert!(config_explicit_padding.encode_padding());
}
