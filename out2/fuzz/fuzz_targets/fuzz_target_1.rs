#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // TODO: 使用 Petri Net 生成的 API 序列
    let _ = data;
});
