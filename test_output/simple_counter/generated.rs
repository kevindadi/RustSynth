//! 由 SyPetype PCPN Simulator 自动生成
//! 
//! 此代码展示了一条通过 Rust 借用检查的 API 调用序列。
//! 所有借用都是显式的，使用 drop() 显式结束借用。

#![allow(unused_variables)]
#![allow(unused_mut)]
#![allow(dead_code)]

fn main() {
    let mut v0 = Counter::new(); // Counter::new
    let mut v1 = Counter::new(); // Counter::new
    let mut v2 = Counter::new(); // Counter::new
    let r3 = &v0; // &Counter [first]
    let mut v4 = Counter::new(); // Counter::new
    let r5 = &v1; // &Counter [first]
    drop(r3); // drop &Counter [unfreeze]
    drop(v2); // drop Counter
    let v6: i32 = 0; // const i32
    drop(r5); // drop &Counter [unfreeze]
}